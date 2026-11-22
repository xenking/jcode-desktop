#!/usr/bin/env python3
"""Drive Context Usage in a hidden native window backed by a private runtime."""

import argparse
import importlib.util
import json
from pathlib import Path
import re
import secrets
import socket
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("verify_jcode_desktop", HERE / "verify-jcode-desktop.py")
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


class ContextUsage(VERIFY.Instance):
    def start_desktop(self, env, modes, suffix=""):
        env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="empty", JCODE_DESKTOP_SCREENSHOT_HIDE_BETA="1")
        env.pop("JCODE_DESKTOP_SCREENSHOT_SWARM", None)
        return super().start_desktop(env, modes, suffix)

    def runtime_spawn(self, name, command):
        if name == "fake-provider":
            self.runtime_env.update(JCODE_MODEL="gpt-5.6-terra", JCODE_OPENAI_COMPAT_DEFAULT_MODEL="gpt-5.6-terra")
        return super().runtime_spawn(name, command)

    def context_usage(self, session):
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(10)
            client.connect(str(self.api_socket))
            with client.makefile("rwb") as stream:
                hello = {"req": "hello", "min_version": 1, "max_version": 1, "client": "desktop-context-acceptance"}
                for id_, request in enumerate((hello, {
                    "req": "session_context_breakdown", "session_id": session,
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
        second = json.loads(self.runtime_debug(f"create_session:{self.profile / 'home'}"))["session_id"]
        for session, count, prompt in [(first, 11042, "First session, short context."),
                                       (second, 224142, "Second session. " * 150)]:
            self.runtime_debug("message", f"[e2e:input-tokens:{count}] {prompt}", "--session", session, "--wait")
        if self.manifest["compact"]:
            for turn in range(4):
                self.runtime_debug("message", f"[e2e:pad] First session, turn {turn}", "--session", first, "--wait")
            self.runtime_debug("message", "First session, keep this recent turn.", "--session", first, "--wait")
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
        assert [usage["total_tokens"] for usage in expected] == [11042, 224142], expected
        assert all(usage["meter"]["source"] == "provider_reported" for usage in expected), expected
        requests = (self.run / "fake-requests.jsonl").read_bytes()
        before = [self.drive("state", session=s)["panel"]["loaded_transcript_items"] for s in (first, second)]
        frames = {}

        def compact_count(tokens):
            for size, suffix in ((1_000_000, "m"), (1_000, "k")):
                if tokens >= size:
                    return f"{tokens / size:.1f}".rstrip("0").rstrip(".") + suffix
            return str(tokens)

        def panel(name, index):
            frame, text = self.capture_text(name)
            conversation = next(bucket["tokens"] for bucket in expected[index]["categories"]
                                if bucket["kind"] == "conversation")
            meter = expected[index]["meter"]
            value = f"{compact_count(meter['occupied_tokens'])}/{compact_count(meter['compaction_threshold_tokens'])}"
            text = text.lower()
            if all(label in text for label in (
                "context usage", "compact now", "shake tools", "conversation", "to compact",
            )) and re.search(rf"\b{compact_count(conversation)}\b", text) and value in re.sub(r"\s+", "", text):
                assert all(label not in text for label in (
                    "estimated breakdown", "model window", "of current context", "higher rates",
                )), text
                if index == 0 and not self.manifest["compact"]:
                    if not (re.search(r"compact now\s*0%", text) and re.search(r"shake tools\s*0%", text)):
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
            assert len(labels) == 1 and "/" in labels[0], text
            assert "auto-compaction" not in text.lower(), text
            return frame

        click("hover:panel-context-meter")
        frames["hover"] = self.runtime_wait(hovered, "one-line native context hover")
        click("panel-context-meter")
        frames["first"] = self.runtime_wait(lambda: panel("first.png", 0), "native first-session breakdown")
        click("context-usage-shake")
        self.drive("press_key", {"key": "fixture:escape"})
        _, closed = self.capture_text("closed.png")
        assert "context usage" not in closed.lower(), closed
        click("panel-context-meter")
        frames["reopened"] = self.runtime_wait(lambda: panel("reopened.png", 0), "native reopened breakdown")
        self.selected(second)
        _, switched = self.capture_text("switched.png")
        assert "context usage" not in switched.lower(), switched
        click("panel-context-meter")
        frames["second"] = self.runtime_wait(lambda: panel("second.png", 1), "native second-session breakdown")
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
        proof = {"breakdowns": expected, "frames": frames}
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
                    fixture="background-jobs", theme=args.theme, window_width=args.width, compact=args.compact)
    instance = instance_type(args.run, manifest)
    try:
        instance.start()
        instance.exercise()
    finally:
        instance.cleanup("Context Usage acceptance run finished")


if __name__ == "__main__":
    main()
