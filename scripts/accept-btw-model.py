#!/usr/bin/env python3
"""Exercise Desktop /btw promotion and reconnect against a private fake provider."""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import secrets
import sys
import tempfile


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("verify_desktop", HERE / "verify-jcode-desktop.py")
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)
PARENT_MODEL = "gpt-5-btw-parent"
CHILD_MODEL = "gpt-5-btw-child"
CHILD_EFFORT = "high"
RECONNECTED_EFFORT = "medium"
QUESTION = "Why was retry removed?"
SENT_QUESTION = re.compile(r"(?:\[[^\]\n]+\] )?" + re.escape(QUESTION))


class Fixture(VERIFY.Instance):
    def start_background_runtime(self, env):
        self.runtime_env = {**env, "JCODE_TEST_SESSION": "1"}
        self.legacy_socket = self.profile / "jcode/legacy.sock"
        self.api_socket = self.profile / "jcode/api.sock"
        port = self.profile / "fake-port"
        self.runtime_env.update(
            JCODE_PROVIDER="openai-compatible", JCODE_MODEL=PARENT_MODEL,
            OPENAI_COMPAT_API_KEY="fixture-only", JCODE_OPENAI_COMPAT_DEFAULT_MODEL=PARENT_MODEL,
            FAKE_LOG=str(self.run / "provider.jsonl"), FAKE_PORT_FILE=str(port),
            FAKE_PREMIUM_MODEL=CHILD_MODEL, JCODE_SOCKET=str(self.legacy_socket),
            JCODE_API_SOCKET=str(self.api_socket),
        )
        (self.profile / "jcode/config.toml").write_text("[server]\nmobile_push = false\n")
        self.runtime_cli = [self.manifest["jcode_bin"], "--no-update", "--no-selfdev"]
        fake = HERE.parent.parent / "jcode/scripts/fake_openai_server.py"
        self.runtime_spawn("fake-provider", [sys.executable, str(fake), "0"])
        self.runtime_wait(lambda: port.is_file() and port.read_text().strip(), "loopback provider")
        self.runtime_env["JCODE_OPENAI_COMPAT_API_BASE"] = f"http://127.0.0.1:{port.read_text().strip()}/v1"
        self.daemon = self.runtime_spawn("private-daemon", [*self.runtime_cli, "serve", "--socket", str(self.legacy_socket)])
        self.runtime_wait(self.legacy_socket.exists, "private daemon")
        self.start_bridge()
        parent = json.loads(self.runtime_debug(f"create_session:{self.profile / 'home'}"))["session_id"]
        env.update(JCODE_TEST_SESSION="1", JCODE_DESKTOP_CONTROL_RUNTIME_SOCKET=str(self.api_socket),
                   JCODE_DESKTOP_CONTROL_RUNTIME_SESSION=parent)

    def start_bridge(self):
        self.api_bridge = self.runtime_spawn("private-bridge", [
            *self.runtime_cli, "api-bridge", "--api-socket", str(self.api_socket),
        ])
        self.runtime_wait(self.api_socket.exists, "private API bridge")

    def reconnect_bridge(self):
        self.runtime_stop(self.api_bridge)
        self.api_socket.unlink(missing_ok=True)
        self.runtime_wait(lambda: self.drive("state")["panel"]["status"].startswith("lost:"),
                          "Desktop detects bridge disconnect")
        self.start_bridge()
        self.runtime_wait(lambda: not self.drive("state")["panel"]["status"].startswith("lost:"),
                          "Desktop reconnects to the replacement bridge")

    def forks(self, parent):
        return {session["id"] for file in (self.profile / "jcode/sessions").glob("session_*.json")
                if (session := json.loads(file.read_text())).get("parent_id") == parent}

    def answer_count(self):
        path = self.run / "provider.jsonl"
        if not path.exists():
            return 0
        return sum(any(m.get("role") == "user" and SENT_QUESTION.fullmatch(str(m.get("content")))
                       for m in request["messages"]) for request in map(json.loads, path.read_text().splitlines()))

    def exercise(self):
        parent = self.target["session_id"]
        self.runtime_wait(lambda: self.drive("state")["panel"]["model"] == PARENT_MODEL, "parent model")
        before = self.drive("state")["panel"]
        assert not self.forks(parent), "the private parent already has a fork"
        self.drive("set_composer", {"text": f"/btw {QUESTION}"})
        self.drive("press_key", {"key": "fixture:enter"})
        self.runtime_wait(lambda: self.answer_count() == 1, "one fake provider request from the side question")
        child = self.runtime_wait(lambda: next(iter(self.forks(parent)), None), "persisted side question child")
        _, _, _, child_info = self.harness_requests([
            ("attach_session", {"session_id": child}),
            ("set_reasoning_effort", {"session_id": child, "effort": CHILD_EFFORT}),
            ("set_model", {"session_id": child, "model": CHILD_MODEL}),
            ("get_runtime_info", {"session_id": child}),
        ])
        expected = identity(child_info)
        assert expected == (CHILD_MODEL, CHILD_EFFORT), child_info
        self.reconnect_bridge()
        self.screenshot("side-question.png")
        self.drive("click", {"selector": "side-question-promote"})
        try:
            self.drive("click", {"selector": "side-question-promote"})
        except RuntimeError as error:
            if '"code": "selector_not_visible"' not in str(error):
                raise
        else:
            raise AssertionError("the promoted card still accepts clicks")
        navigation = self.drive("list")["navigation"]
        assert sum(panel["session"] == child for row in navigation["rows"] for panel in row["panels"]) == 1
        self.target["session_id"] = child
        promoted = self.drive("state")["panel"]
        VERIFY.save(self.run / "promoted.json", promoted)
        def promoted_history():
            return any(QUESTION in str(item.get("User", ""))
                       for item in self.drive("state")["panel"]["loaded_transcript_items"]
                       if isinstance(item, dict))

        self.runtime_wait(promoted_history, "original question is present in the promoted chat")
        self.screenshot("promoted.png")
        assert identity(promoted) == expected, f"promotion lost child identity: {identity(promoted)!r} != {expected!r}"
        assert identity(self.drive("state", session=parent)["panel"]) == identity(before)

        self.harness_requests([
            ("attach_session", {"session_id": child}),
            ("set_reasoning_effort", {"session_id": child, "effort": RECONNECTED_EFFORT}),
        ])
        self.reconnect_bridge()

        def reconnected_panel():
            panel = self.drive("state")["panel"]
            if identity(panel) == (CHILD_MODEL, RECONNECTED_EFFORT) and not panel["status"].startswith("lost:"):
                return panel

        reconnected = self.runtime_wait(reconnected_panel, "child identity after reconnect")
        VERIFY.save(self.run / "reconnected.json", reconnected)
        assert self.answer_count() == 1, "promotion or reconnect sent another provider request"
        assert self.forks(parent) == {child}, "promotion or reconnect forked again"
        return {"parent": parent, "child": child, "model": expected[0], "effort": expected[1],
                "reconnected_effort": reconnected["reasoning_effort"], "requests": 1}


def identity(panel):
    return panel["model"], panel["reasoning_effort"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--jcode", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    VERIFY.require_safe_host()
    run = args.evidence.resolve()
    run.mkdir(mode=0o700, parents=True)
    with tempfile.TemporaryDirectory(prefix="jdb-", dir="/tmp") as directory:
        scratch = Path(directory).resolve()
        token = secrets.token_hex(16)
        owner = scratch / ".owner"
        owner.write_text(token)
        owner.chmod(0o600)
        (scratch / "control").mkdir(mode=0o700)
        manifest = {"schema": VERIFY.SCHEMA, "evidence": str(run), "phase": "starting",
                    "scratch": str(scratch), "token": token, "fixture": "swarm-dm",
                    "binary": str(args.desktop.resolve()), "jcode_bin": str(args.jcode.resolve())}
        fixture = Fixture(run, manifest)
        try:
            fixture.start()
            proof = fixture.exercise()
            VERIFY.save(run / "btw-proof.json", proof)
            print(json.dumps(proof))
        finally:
            fixture.cleanup("side question model acceptance finished")


if __name__ == "__main__":
    main()
