#!/usr/bin/env python3
"""Drive Context Usage in a hidden native window backed by a private runtime."""

import argparse
from datetime import datetime, timezone
import importlib.util
import json
from pathlib import Path
import re
import secrets
import socket
import subprocess
import tempfile
import time


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("verify_jcode_desktop", HERE / "verify-jcode-desktop.py")
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


class ContextUsage(VERIFY.Instance):
    def start_desktop(self, env, modes, suffix=""):
        env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="empty", JCODE_DESKTOP_SCREENSHOT_HIDE_BETA="1",
                   JCODE_DESKTOP_SCREENSHOT_CONTEXT_OVERLAY_PROOF="1")
        env.pop("JCODE_DESKTOP_SCREENSHOT_SWARM", None)
        return super().start_desktop(env, modes, suffix)

    def runtime_spawn(self, name, command):
        if name == "fake-provider":
            self.runtime_env.update(JCODE_MODEL="gpt-5.6-terra", JCODE_OPENAI_COMPAT_DEFAULT_MODEL="gpt-5.6-terra",
                                    JCODE_MEMORY_ENABLED="false", JCODE_MEMORY_SIDECAR_ENABLED="false")
        return super().runtime_spawn(name, command)

    def start_background_runtime(self, env):
        if self.manifest["snapcompact"]:
            self.second = "session_fox_1795279000002_snapcompact"
            now = datetime.now(timezone.utc).isoformat()
            sessions = self.profile / "jcode/sessions"
            sessions.mkdir(exist_ok=True)
            VERIFY.save(sessions / f"{self.second}.json", dict(
                id=self.second, title="Snapcompact fixture", created_at=now, updated_at=now,
                model="gpt-5.6-terra", provider_key="openai-compatible", working_dir=str(self.profile / "home"),
                messages=[dict(id="saved-tail", role="user", timestamp=now,
                               content=[dict(type="text", text="Keep the recent tail.")]),
                          dict(id="saved-answer", role="assistant", timestamp=now,
                               content=[dict(type="text", text="The recent tail is intact.")])],
                compaction=dict(summary_text="Saved fixture checkpoint", covers_up_to_turn=0,
                                original_turn_count=0, compacted_count=0, snapcompact=dict(
                                    source="older input kept in the archive", pages=[],
                                    index="[message 0, page 1 | user] indexed old fact\n",
                                    decisions="A checkpoint decision kept in the outbound request.",
                                    deferred_decisions="Archive-only decision. " * 200,
                                    page_ranges=[], render_key="")),
            ))
        super().start_background_runtime(env)
        first = env["JCODE_DESKTOP_CONTROL_RUNTIME_SESSION"]
        if not self.manifest["snapcompact"]:
            self.second = json.loads(self.runtime_debug(f"create_session:{self.profile / 'home'}"))["session_id"]
        self.runtime_debug("message", "[e2e:pad] [e2e:input-tokens:11042] First session with long history.",
                           "--session", first, "--wait")
        threshold = self.context_usage(first)["meter"]["compaction_threshold_tokens"]
        self.second_total = round(threshold * .35)
        second_prompt = f"[e2e:input-tokens:{self.second_total}] " + "Second session. " * 150
        if self.manifest["snapcompact"]:
            with socket.socket(socket.AF_UNIX) as client:
                client.settimeout(10)
                client.connect(str(self.legacy_socket))
                subscribe = {"type": "subscribe", "id": 1, "target_session_id": self.second,
                             "working_dir": str(self.profile / "home")}
                client.sendall(json.dumps(subscribe).encode() + b"\n")
                with client.makefile("rb") as stream:
                    for line in stream:
                        event = json.loads(line)
                        if event.get("id") == 1 and event["type"] in ("error", "done"):
                            break
                    else:
                        raise RuntimeError("Runtime closed before opening the saved checkpoint")
                assert event["type"] == "done", event
                self.runtime_debug("message", second_prompt, "--session", self.second, "--wait")
        else:
            self.runtime_debug("message", second_prompt, "--session", self.second, "--wait")
        self.first_total = 11_042
        if self.manifest["compact"]:
            self.first_total = 57_665
            for turn in range(4):
                self.runtime_debug("message", f"[e2e:pad] First session, turn {turn}", "--session", first, "--wait")
            self.runtime_debug("message", f"[e2e:input-tokens:{self.first_total}] First session, keep this recent turn.",
                               "--session", first, "--wait")

    def context_usage(self, session):
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(10)
            client.connect(str(self.api_socket))
            with client.makefile("rwb") as stream:
                hello = {"req": "hello", "min_version": 1, "max_version": 1, "client": "desktop-context-acceptance"}
                for id_, request in enumerate((hello, {
                    "req": "session_context_breakdown", "session_id": session,
                    "include_snapcompact": True,
                }), 1):
                    stream.write(json.dumps({"v": 1, "id": id_, **request}).encode() + b"\n")
                    stream.flush()
                    while True:
                        reply = json.loads(stream.readline())
                        if reply.get("reply_to") == id_:
                            break
        assert reply["ev"] == "context_breakdown", reply
        result = {key: value for key, value in reply.items() if key not in ("v", "ev", "reply_to")}
        assert result["session_id"] == session and result["limit_tokens"] > 0, result
        assert result["categories"], result
        meter = result["meter"]
        assert meter["occupied_tokens"] == result["total_tokens"], result
        assert meter["compaction_threshold_tokens"] < meter["model_window_tokens"], result
        return result

    def capture_text(self, name):
        name = f"{Path(name).stem}-{self.sequence}.png"
        frame = self.screenshot(name)
        text = subprocess.check_output(["tesseract", frame["path"], "stdout", "--psm", "11"],
                                       text=True, stderr=subprocess.DEVNULL)
        (self.run / name.replace(".png", ".txt")).write_text(text)
        return frame, text

    def selected(self, session):
        self.drive("focus", session=session)
        self.target["session_id"] = session

    def exercise(self):
        first = self.target["session_id"]
        second = self.second
        def open_second():
            try:
                self.drive("open_session", {"session_id": second})
                return True
            except RuntimeError as error:
                if "session_not_found" not in str(error):
                    raise
                return False

        self.runtime_wait(open_second, "second private session in Desktop's catalog")
        self.selected(first)
        expected = [self.context_usage(session) for session in (first, second)]
        assert [usage["total_tokens"] for usage in expected] == [self.first_total, self.second_total], expected
        assert all(usage["meter"]["source"] == "provider_reported" for usage in expected), expected
        if self.manifest["snapcompact"]:
            checkpoint = next(bucket["tokens"] for bucket in expected[1]["categories"]
                              if bucket["kind"] == "snapcompact")
            assert checkpoint > 0, expected[1]
        requests = (self.run / "fake-requests.jsonl").read_bytes()
        before = [self.drive("state", session=s)["panel"]["loaded_transcript_items"] for s in (first, second)]
        frames = {}

        def compact_count(tokens):
            for size, suffix in ((1_000_000, "m"), (1_000, "k")):
                if tokens >= size:
                    return f"{tokens / size:.1f}".rstrip("0").rstrip(".") + suffix
            return str(tokens)

        def meter_value(index):
            meter = expected[index]["meter"]
            used = compact_count(meter["occupied_tokens"])
            limit = compact_count(meter["compaction_threshold_tokens"])
            return f"{used}/{limit}"

        def panel(name, index):
            frame, text = self.capture_text(name)
            conversation = next(bucket["tokens"] for bucket in expected[index]["categories"]
                                if bucket["kind"] == "conversation")
            text = text.lower()
            required = ("context usage", "compact now", "shake tools", "conversation", "to compact")
            if index == 1 and self.manifest["snapcompact"]:
                required += ("snapcompact checkpoint", compact_count(checkpoint))
            seen_conversation = re.search(rf"\b{re.escape(compact_count(conversation))}\b", text)
            if (all(label in text for label in required) and seen_conversation
                    and meter_value(index) in re.sub(r"\s+", "", text)):
                assert all(label not in text for label in (
                    "estimated breakdown", "model window", "of current context", "higher rates",
                )), text
                if index == 0 and not self.manifest["compact"]:
                    forecast = expected[index].get("action_forecast") or {}
                    for action, label in (("compact", "compact now"), ("shake", "shake tools")):
                        if forecast.get(action) and not re.search(rf"{label}\D*[0-9][0-9.\s]*%", text):
                            return None
                return frame
            return None

        def click(selector):
            self.drive("click", {"selector": selector})

        def hovered():
            frame, text = self.capture_text("hover.png")
            labels = [line for line in text.lower().splitlines() if "context usage" in line]
            if not labels:
                return None
            assert len(labels) == 1 and meter_value(0) in re.sub(r"\s+", "", labels[0]), text
            assert "auto-compaction" not in text.lower(), text
            return frame

        def geometry():
            state = self.drive("state")["panel"]
            return {"scroll": state["scroll_fixture"], "editor": state["composer_fixture"],
                    "draft": state["composer"]}

        short = geometry()
        draft = "Long draft stays here.\n" * 70
        self.drive("set_composer", {"text": draft})
        self.screenshot("draft-ready.png")  # Layout the selected hidden window before dispatching the wheel.
        click("history:scroll-up")
        click("scroll:transcript")
        deadline = time.monotonic() + 5
        while True:
            before_overlay = geometry()
            if (before_overlay["scroll"]["offset_y"] < 0
                    and before_overlay["editor"]["editor_height"] > short["editor"]["editor_height"]):
                break
            assert time.monotonic() < deadline, before_overlay
            time.sleep(.05)
        time.sleep(.3)  # Let the native wheel animation settle before taking the baseline.
        before_overlay = geometry()
        assert before_overlay["draft"] == draft and before_overlay["scroll"]["offset_y"] < 0, before_overlay
        frames["before_overlay"] = self.screenshot("before-overlay.png")
        click("panel-context-meter")
        frames["first"] = self.runtime_wait(lambda: panel("first.png", 0), "native first-session breakdown")
        with_overlay = geometry()
        assert with_overlay == before_overlay, (before_overlay, with_overlay)
        click("panel-context-meter")
        frames["after_overlay"] = self.screenshot("after-overlay.png")
        after_overlay = geometry()
        assert after_overlay == before_overlay, (before_overlay, after_overlay)
        self.drive("set_composer", {"text": ""})
        click("panel-context-meter")
        self.runtime_wait(lambda: panel("after-clear.png", 0), "native overlay after clearing the draft")
        click("context-usage-shake")
        self.drive("press_key", {"key": "fixture:escape"})
        _, closed = self.capture_text("closed.png")
        assert "context usage" not in closed.lower(), closed
        meter = expected[0]["meter"]
        percent = 100 * meter["occupied_tokens"] / meter["compaction_threshold_tokens"]
        assert f"{percent:.0f}%" in closed and meter_value(0) not in re.sub(r"\s+", "", closed), closed
        click("hover:panel-context-meter")
        frames["hover"] = self.runtime_wait(hovered, "one-line native context hover")
        click("panel-context-meter")
        frames["reopened"] = self.runtime_wait(lambda: panel("reopened.png", 0), "native reopened breakdown")
        self.selected(second)
        _, switched = self.capture_text("switched.png")
        assert "context usage" not in switched.lower(), switched
        click("panel-context-meter")
        frames["second"] = self.runtime_wait(lambda: panel("second.png", 1), "native second-session breakdown")
        click("panel-context-meter")
        click("proof-pointer:16,140")
        frames["ring"], ring_text = self.capture_text("ring-35-percent.png")
        assert "35%" in ring_text and "context usage" not in ring_text.lower(), ring_text
        self.selected(first)
        _, returned = self.capture_text("return-arrival.png")
        if "context usage" not in returned.lower():
            # When both sessions stayed mounted, clicking the other session's
            # meter was outside the first panel and dismissed its breakdown.
            click("panel-context-meter")
        frames["return"] = self.runtime_wait(lambda: panel("return.png", 0), "native first-session breakdown after return")
        assert expected == [self.context_usage(session) for session in (first, second)]
        assert requests == (self.run / "fake-requests.jsonl").read_bytes()
        assert before == [self.drive("state", session=s)["panel"]["loaded_transcript_items"] for s in (first, second)]
        proof = {"breakdowns": expected, "frames": frames,
                 "overlay_geometry": {"before": before_overlay["scroll"], "open": with_overlay["scroll"],
                                      "closed": after_overlay["scroll"], "editor": before_overlay["editor"]}}
        if self.manifest["compact"]:
            def compacted():
                frame, text = self.capture_text("compacted.png")
                if "context compacted in this" in text.lower() and "local estimate" in text.lower():
                    return frame
                if "cannot compact" in text.lower() or "compaction failed" in text.lower():
                    raise RuntimeError(text)
                return None

            click("context-usage-compact")
            frames["compact"] = self.runtime_wait(compacted, "selected-session native Compact result")
            proof["after_compact"] = self.context_usage(first)
            assert self.context_usage(second) == expected[1]
            assert self.drive("state", session=second)["panel"]["loaded_transcript_items"] == before[1]
        proof["passed"] = True
        VERIFY.save(self.run / "context-usage-proof.json", proof)


def main(instance_type=ContextUsage):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--desktop-bin", type=Path, required=True)
    parser.add_argument("--jcode-bin", type=Path, required=True)
    parser.add_argument("--bridge-bin", type=Path, required=True)
    parser.add_argument("--width", type=int, choices=range(360, 1601), metavar="360..1600", required=True)
    parser.add_argument("--theme", choices=("neutral-light", "neutral-dark"), required=True)
    parser.add_argument("--compact", action="store_true", help="also apply Compact to the first private session")
    parser.add_argument("--snapcompact", action="store_true", help="show an active snapcompact checkpoint in the second session")
    args = parser.parse_args()
    VERIFY.require_safe_host()
    args.run = args.run.resolve()
    args.run.mkdir(mode=0o700)
    scratch = Path(tempfile.mkdtemp(prefix="jdc-", dir="/private/tmp"))
    token = secrets.token_hex(32)
    owner = scratch / ".owner"
    owner.write_text(token)
    owner.chmod(0o600)
    manifest = dict(schema=VERIFY.SCHEMA, evidence=str(args.run), scratch=str(scratch), token=token,
                    binary=str(args.desktop_bin.resolve()), jcode_bin=str(args.jcode_bin.resolve()),
                    bridge_bin=str(args.bridge_bin.resolve()), sources=VERIFY.source_identity(),
                    fixture="background-jobs", theme=args.theme, window_width=args.width, compact=args.compact,
                    snapcompact=args.snapcompact)
    instance = instance_type(args.run, manifest)
    try:
        instance.start()
        instance.exercise()
    finally:
        instance.cleanup("Context Usage acceptance run finished")


if __name__ == "__main__":
    main()
