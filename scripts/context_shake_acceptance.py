#!/usr/bin/env python3
"""Exercise manual Shake through a hidden native Desktop and private daemon/bridge."""

from datetime import datetime, timezone
import json
from pathlib import Path
import sys

from context_usage_acceptance import ContextUsage, VERIFY, main


FIRST = "session_owl_1795279000000_shake_first"
SECOND = "session_fox_1795279000001_shake_second"


def seed(directory, working_dir, session, eligible):
    now = datetime.now(timezone.utc).isoformat()
    text = lambda value: {"type": "text", "text": value}
    messages = [
        ("user", text("Preserve this user text and its fenced code:\n```text\n" + "literal source\n" * 800 + "```")),
        ("assistant", {"type": "tool_use", "id": "old-read", "name": "read", "input": {"file_path": "old.txt"}}),
        ("user", {"type": "tool_result", "tool_use_id": "old-read", "content": "old fixture output\n" * (1_200 if eligible else 10)}),
        ("user", text("Preserve the recent turn.") if not eligible else text("Recent text\n" * 6_000)),
        ("assistant", text("Recent response stays.")),
    ]
    stored = [dict(id=f"fixture-{i}", role=role, content=[block], timestamp=now)
              for i, (role, block) in enumerate(messages)]
    used = 26_000 if eligible else 7_000
    stored[-1]["token_usage"] = dict(prompt_tokens=used, input_tokens=used, output_tokens=6, cache_read_input_tokens=2_000)
    directory.mkdir(exist_ok=True)
    VERIFY.save(directory / f"{session}.json", dict(
        id=session, title=session, created_at=now, updated_at=now, messages=stored,
        model="gpt-5.6-terra", provider_key="openai-compatible", working_dir=str(working_dir),
        last_provider_request_at=now,
    ))


class ShakeAcceptance(ContextUsage):
    def start_background_runtime(self, env):
        self.runtime_env = {**env, "JCODE_TEST_SESSION": "1"}
        self.legacy_socket = self.profile / "jcode/legacy.sock"
        self.api_socket = self.profile / "jcode/api.sock"
        self.runtime_env.update(
            JCODE_PROVIDER="openai-compatible", JCODE_MODEL="gpt-5.6-terra",
            JCODE_MEMORY_ENABLED="false", JCODE_MEMORY_SIDECAR_ENABLED="false",
            OPENAI_COMPAT_API_KEY="fixture-only", JCODE_OPENAI_COMPAT_DEFAULT_MODEL="gpt-5.6-terra",
            FAKE_LOG=str(self.run / "fake-requests.jsonl"), FAKE_CONTROL_DIR=str(self.profile),
            FAKE_PORT_FILE=str(self.profile / "fake-port"),
            JCODE_SOCKET=str(self.legacy_socket), JCODE_API_SOCKET=str(self.api_socket),
        )
        (self.profile / "jcode/config.toml").write_text(
            "[server]\nmobile_push = false\n[compaction]\nshake = false\nshake_cache_expiry = false\ncache_ttl_secs = 3600\n"
        )
        for session in (FIRST, SECOND):
            seed(self.profile / "jcode/sessions", self.profile / "home", session, session == FIRST)
        self.runtime_cli = [self.manifest["jcode_bin"], "--no-update", "--no-selfdev"]
        provider = VERIFY.REPO.parent / "jcode/scripts/fake_openai_server.py"
        self.runtime_spawn("fake-provider", [sys.executable, str(provider), "0"])
        port_file = Path(self.runtime_env["FAKE_PORT_FILE"])
        self.runtime_wait(port_file.is_file, "isolated loopback sentinel")
        self.runtime_env["JCODE_OPENAI_COMPAT_API_BASE"] = f"http://127.0.0.1:{port_file.read_text().strip()}/v1"
        self.start_background_daemon()
        env.update(JCODE_TEST_SESSION="1", JCODE_DESKTOP_CONTROL_RUNTIME_SOCKET=str(self.api_socket),
                   JCODE_DESKTOP_CONTROL_RUNTIME_SESSION=FIRST)

    def exercise(self):
        assert self.target["session_id"] == FIRST, self.target
        def open_second():
            try:
                self.drive("open_session", {"session_id": SECOND})
                return True
            except RuntimeError as error:
                if "session_not_found" not in str(error):
                    raise
                return False

        self.runtime_wait(open_second, "the second private session in Desktop's catalog")
        self.selected(FIRST)
        draft = "This private unsent draft must survive Shake."
        self.drive("set_composer", {"text": draft})
        assert self.drive("state")["panel"]["composer"] == draft
        before = {s: self.context_usage(s) for s in (FIRST, SECOND)}
        stored = self.profile / "jcode/sessions"
        second_before = {p.name: p.read_bytes() for p in stored.glob(f"{SECOND}.*")}
        fake_requests = self.run / "fake-requests.jsonl"
        first_path = stored / f"{FIRST}.json"
        original = json.loads(first_path.read_text())["messages"]
        assert not fake_requests.exists() or not fake_requests.read_bytes(), "fixture startup called the provider"

        def click(selector):
            self.drive("click", {"selector": selector})

        def visible(name, *expected):
            frame, text = self.capture_text(f"{name}.png")
            normalized = " ".join(text.lower().split())
            return frame if all(label in normalized for label in expected) else None

        click("panel-context-meter")
        forecast = before[FIRST]["action_forecast"]
        assert 0 < forecast["shake"]["tokens"] < forecast["current_tokens"], forecast
        self.runtime_wait(lambda: visible("before", "context usage", "shake tools", "of current context", "est."), "native Shake forecast before clicking")
        click("context-usage-shake")
        preview = self.runtime_wait(lambda: visible("preview", "confirm shake", "cache"), "native warm-cache Shake preview and confirm button")
        assert before == {s: self.context_usage(s) for s in (FIRST, SECOND)}, "preview changed context"
        click("context-usage-shake-cancel")
        assert before == {s: self.context_usage(s) for s in (FIRST, SECOND)}, "cancel changed context"
        click("context-usage-shake")
        self.runtime_wait(lambda: visible("confirm", "confirm shake", "cache"), "new preview after cancel")
        click("context-usage-shake-confirm")
        applied = self.runtime_wait(lambda: visible("applied", "local estimate", "% saved"), "native measured Shake savings")
        after = {s: self.context_usage(s) for s in (FIRST, SECOND)}
        assert after[FIRST]["action_forecast"]["current_tokens"] < forecast["current_tokens"], (before, after)
        assert after[SECOND] == before[SECOND]
        assert self.drive("state")["panel"]["composer"] == draft
        written = {m["id"]: m for m in json.loads(first_path.read_text())["messages"]}
        for message in original:
            actual = written[message["id"]]["content"]
            expected = message["content"]
            if expected[0]["type"] == "tool_result":
                assert actual[0]["tool_use_id"] == expected[0]["tool_use_id"]
                assert actual[0]["content"].startswith("[elided ")
            else:
                assert actual == expected, message["id"]
        assert {p.name: p.read_bytes() for p in stored.glob(f"{SECOND}.*")} == second_before
        click("context-usage-shake")
        repeated = self.runtime_wait(lambda: visible("repeated", "no eligible tool outputs"), "native repeated Shake no-op")
        assert self.context_usage(FIRST) == after[FIRST]
        assert not fake_requests.exists() or not fake_requests.read_bytes(), "a provider request escaped the Shake workflow"
        VERIFY.save(self.run / "shake-proof.json", dict(before=before, after=after, preview=preview, applied=applied, repeated=repeated, provider_requests=0))


if __name__ == "__main__":
    main(ShakeAcceptance)
