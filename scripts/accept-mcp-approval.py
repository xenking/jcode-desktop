#!/usr/bin/env python3
"""Answer an MCP approval prompt in a hidden Desktop window, allow and deny.

Launches a private daemon whose MCP server uses `approval = "annotations"`, a
scripted provider (from the jcode repository's accept-mcp-typed.py) and the
real Desktop binary in a hidden window. One codemode program makes three gated
writes. The script answers the prompts through Desktop's native control socket:
the first with Allow once, the second with Deny, the third with Allow once
again after a reselect. It checks the card text, that a background request is
refused while a prompt is open, that only the allowed writes reached the
server (by its write log), and keeps a screenshot of each prompt and of the
finished transcript.

Usage: accept-mcp-approval.py --desktop BIN --jcode BIN --evidence DIR
"""
import argparse
import importlib.util
import json
from pathlib import Path
import secrets
import tempfile

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("verify_desktop", HERE / "verify-jcode-desktop.py")
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)
JCODE_ACCEPT = importlib.util.spec_from_file_location(
    "accept_mcp_typed", HERE.parent.parent / "jcode/scripts/accept-mcp-typed.py")
TYPED = importlib.util.module_from_spec(JCODE_ACCEPT)
JCODE_ACCEPT.loader.exec_module(TYPED)

ALLOW, DENY = 1, 0  # option order of the card: Deny, Allow once


class Fixture(VERIFY.Instance):
    def start_background_runtime(self, env):
        self.runtime_env = {**env, "JCODE_TEST_SESSION": "1"}
        self.legacy_socket = self.profile / "jcode/legacy.sock"
        self.api_socket = self.profile / "jcode/api.sock"
        self.writes = self.run / "mcp-writes.log"
        server = self.profile / "mcp_fixture.py"
        server.write_text(TYPED.STDIO_SERVER)
        (self.profile / "jcode/mcp.json").write_text(json.dumps({"servers": {"fixture": {
            "command": "python3", "args": ["-I", "-S", "-u", str(server)], "shared": False, "timeout_secs": 20,
            "env": {"WRITE_LOG": str(self.writes)}, "approval": "annotations"}}}))
        self.provider, _ = TYPED.start_provider(TYPED.SCRIPT_APPROVAL)
        self.runtime_env.update(
            JCODE_PROVIDER="openai-compatible", JCODE_MODEL="fake-model", WRITE_LOG=str(self.writes),
            OPENAI_COMPAT_API_KEY="fixture-only", JCODE_OPENAI_COMPAT_DEFAULT_MODEL="fake-model",
            JCODE_OPENAI_COMPAT_API_BASE=f"http://127.0.0.1:{self.provider.server_port}/v1",
            JCODE_DISABLE_CLAUDE_MCP="1", JCODE_SOCKET=str(self.legacy_socket), JCODE_API_SOCKET=str(self.api_socket),
        )
        (self.profile / "jcode/config.toml").write_text("[server]\nmobile_push = false\n")
        self.runtime_cli = [self.manifest["jcode_bin"], "--no-update", "--no-selfdev"]
        self.daemon = self.runtime_spawn("private-daemon", [*self.runtime_cli, "serve", "--socket", str(self.legacy_socket)])
        self.runtime_wait(self.legacy_socket.exists, "private daemon")
        self.api_bridge = self.runtime_spawn("private-bridge", [*self.runtime_cli, "api-bridge", "--api-socket", str(self.api_socket)])
        self.runtime_wait(self.api_socket.exists, "private API bridge")
        session = json.loads(self.runtime_debug(f"create_session:{self.profile / 'home'}"))["session_id"]
        env.update(JCODE_TEST_SESSION="1", JCODE_DESKTOP_CONTROL_RUNTIME_SOCKET=str(self.api_socket),
                   JCODE_DESKTOP_CONTROL_RUNTIME_SESSION=session)

    def write_lines(self):
        return self.writes.read_text().splitlines() if self.writes.exists() else []

    def ask(self, index):
        def pending():
            ask = self.drive("state")["panel"]["ask"]
            if ask and ask["request_id"].startswith("ask-mcp-approval-") and f'"n":{index}' in ask["questions"][0]["question"] \
                    and ask["option_selectors"] and not ask["submitting"]:
                return ask
        return self.runtime_wait(pending, f"approval prompt {index} in Desktop")

    def answer(self, index, option, label):
        ask = self.ask(index)
        question = ask["questions"][0]
        assert question.get("header") == "MCP approval", question
        assert '"write-item"' in question["question"] and '"fixture"' in question["question"], question
        assert [o["label"] for o in question["options"]] == ["Deny", "Allow once"], question["options"]
        self.screenshot(f"approval-{index}-{label}.png")
        self.drive("click", {"selector": ask["option_selectors"][option]})
        chosen = self.drive("state")["panel"]["ask"]["chosen"]
        assert chosen and chosen[0] == [option], chosen
        self.drive("press_key", {"key": "fixture:enter"})
        return ask["request_id"]

    def exercise(self):
        self.runtime_wait(lambda: not self.drive("state")["panel"]["status"].startswith("lost:"), "Desktop attached")
        self.drive("set_composer", {"text": "call the mcp tools"})
        self.drive("press_key", {"key": "fixture:enter"})

        first = self.ask(1)
        assert self.drive("state")["panel"]["ask"]["request_id"] == first["request_id"]
        # A background request from another client must leave the open prompt in place.
        reply = self.harness_requests([("attach_session", {"session_id": self.target["session_id"]}), ("background_tool", {})])[-1]
        moved = reply.get("moved", reply.get("result", {}).get("moved"))
        assert moved is False, reply
        assert self.ask(1)["request_id"] == first["request_id"], "the prompt went away"

        self.answer(1, ALLOW, "allow")
        self.answer(2, DENY, "deny")
        self.answer(3, ALLOW, "allow-again")

        def finished():
            items = self.drive("state")["panel"]["loaded_transcript_items"]
            text = json.dumps(items)
            return text if "write3" in text and "mcp__fixture__write_item" in text else None

        self.runtime_wait(finished, "codemode result in the transcript")
        self.screenshot("final-transcript.png")
        assert self.write_lines() == ['write-item:{"n": 1}', 'write-item:{"n": 3}'], self.write_lines()
        text = json.dumps(self.drive("state")["panel"]["loaded_transcript_items"])
        assert "denied by the user" in text, "the denial is not visible in the transcript"
        return {"writes": self.write_lines(), "answers": ["allow", "deny", "allow"], "background_request": reply, "passed": True}

    def cleanup(self, reason):
        if getattr(self, "provider", None):
            self.provider.shutdown()
        super().cleanup(reason)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--jcode", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    VERIFY.require_safe_host()
    run = args.evidence.resolve()
    run.mkdir(mode=0o700, parents=True)
    with tempfile.TemporaryDirectory(prefix="jdm-", dir="/tmp") as directory:
        scratch = Path(directory).resolve()
        token = secrets.token_hex(16)
        owner = scratch / ".owner"
        owner.write_text(token)
        owner.chmod(0o600)
        (scratch / "control").mkdir(mode=0o700)
        manifest = {"schema": VERIFY.SCHEMA, "evidence": str(run), "phase": "starting",
                    "scratch": str(scratch), "token": token, "fixture": "swarm-dm",
                    "binary": str(args.desktop.resolve()), "jcode_bin": str(args.jcode.resolve()),
                    "sources": VERIFY.source_identity()}
        fixture = Fixture(run, manifest)
        try:
            fixture.start()
            proof = fixture.exercise()
            VERIFY.save(run / "mcp-approval-proof.json", proof)
            print(json.dumps(proof))
        finally:
            fixture.cleanup("mcp approval acceptance finished")


if __name__ == "__main__":
    main()
