#!/usr/bin/env python3
"""Drive the session footer in hidden macOS Desktop with a private loopback runtime."""
import argparse
from http.server import ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import secrets
import tempfile
import threading
import time


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


HERE = Path(__file__).resolve().parent
VERIFY = load(HERE / "verify-jcode-desktop.py", "verify_desktop")
ASK = load(HERE.parent.parent / "jcode/scripts/accept-ask-reload.py", "ask_runtime")


class Provider(ASK.Provider):
    steps = []

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["content-length"])))
        messages = request["messages"]
        user, turn = next((i, label) for i in range(len(messages) - 1, -1, -1)
                          for label in ("Choose one", "Choose again", "Continue after cancellation")
                          if messages[i]["role"] == "user" and label in str(messages[i]["content"]))
        results = sum(message["role"] == "tool" for message in messages[user + 1:])
        self.steps.append({"turn": turn, "tool_results": results,
                           "roles": [message["role"] for message in messages[user + 1:]],
                           "last_tool": next((message["content"] for message in reversed(messages)
                                              if message["role"] == "tool"), None),
                           "time": time.monotonic()})
        if turn == "Continue after cancellation":
            delta, finish = {"role": "assistant", "content": "The next turn completed."}, "stop"
        elif results >= 2:
            delta, finish = {"role": "assistant", "content": "Footer test completed."}, "stop"
        else:
            name, arguments = (("ask", {"intent": "verify footer", "questions": [
                {"question": "Choose?", "options": [{"label": "Alpha"}, {"label": "Beta"}]},
            ]}) if results == 0 else ("bash", {"intent": "hold the tool state for a frame", "command": "sleep 3"}))
            delta = {"role": "assistant", "tool_calls": [{"index": 0, "id": f"footer-{name}-{turn.split()[-1]}",
                      "type": "function", "function": {"name": name, "arguments": json.dumps(arguments)}}]}
            finish = "tool_calls"
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        for choice in ({"index": 0, "delta": delta}, {"index": 0, "delta": {}, "finish_reason": finish}):
            chunk = {"id": "footer-fixture", "object": "chat.completion.chunk", "model": "fake-model", "choices": [choice]}
            self.wfile.write(b"data: " + json.dumps(chunk).encode() + b"\n\n")
        self.wfile.write(b"data: [DONE]\n\n")


class Fixture(VERIFY.Instance):
    def start_background_runtime(self, env):
        self.runtime_env = {**env, "JCODE_TEST_SESSION": "1"}
        self.legacy_socket = self.profile / "jcode/legacy.sock"
        self.api_socket = self.profile / "jcode/api.sock"
        self.runtime_env.update(
            JCODE_PROVIDER="openai-compatible", JCODE_MODEL="fake-model",
            OPENAI_COMPAT_API_KEY="fixture-only", JCODE_OPENAI_COMPAT_DEFAULT_MODEL="fake-model",
            JCODE_OPENAI_COMPAT_API_BASE=self.manifest["provider"],
            JCODE_SOCKET=str(self.legacy_socket), JCODE_API_SOCKET=str(self.api_socket),
        )
        (self.profile / "jcode/config.toml").write_text("[server]\nmobile_push = false\n")
        self.runtime_cli = [self.manifest["jcode_bin"], "--no-update", "--no-selfdev"]
        self.daemon = self.runtime_spawn("private-daemon", [*self.runtime_cli, "serve", "--socket", str(self.legacy_socket)])
        self.runtime_wait(self.legacy_socket.exists, "private daemon")
        self.api_bridge = self.runtime_spawn("private-bridge", [
            *self.runtime_cli, "api-bridge", "--api-socket", str(self.api_socket),
        ])
        self.runtime_wait(self.api_socket.exists, "private API bridge")
        session_id = json.loads(self.runtime_debug(f"create_session:{self.profile / 'home'}"))["session_id"]
        env.update(JCODE_TEST_SESSION="1", JCODE_DESKTOP_CONTROL_RUNTIME_SOCKET=str(self.api_socket),
                   JCODE_DESKTOP_CONTROL_RUNTIME_SESSION=session_id)

    def start_desktop(self, env, modes, suffix=""):
        self.test_env, self.test_modes = env, modes
        super().start_desktop(env, modes, suffix)

    def visible(self, selector):
        try:
            self.drive("click", {"selector": selector})
            return True
        except RuntimeError as error:
            if '"code": "selector_not_visible"' not in str(error):
                raise
            return False

    def frame(self, state, name=None):
        selector = f"session-status-{state}"
        self.runtime_wait(lambda: self.visible(selector), selector)
        return self.screenshot((name or state) + ".png")

    def exercise(self):
        frames = {"idle": self.frame("idle")}
        self.drive("set_composer", {"text": "Choose one"})
        self.drive("send")
        frames["ask"] = self.frame("ask")
        assert not self.visible("session-status-tools"), "Ask duplicated the tool status"
        assert not self.visible("transcript-activity"), "Ask duplicated the current transcript status"
        state = self.drive("state")["panel"]
        ask = state["ask"]
        self.drive("click", {"selector": f"ask:{ask['request_id']}:0:1"})
        self.drive("press_key", {"key": "fixture:enter"})
        frames["tools-after-ask"] = self.frame("tools", "tools-after-ask")
        assert self.visible("transcript-activity"), "real tool activity did not return after answering Ask"
        frames["done"] = self.frame("done")
        assert not self.visible("session-status-idle"), "final did not survive transport idle"
        sid = self.target["session_id"]
        self.drive("set_composer", {"text": "Choose again"})
        self.drive("send")
        self.frame("ask", "ask-before-cancel")
        client = ASK.Client(str(self.legacy_socket))
        try:
            client.send(type="subscribe", id=1, target_session_id=sid,
                        working_dir=str(self.profile / "home"), continue_on_disconnect=True)
            client.until(lambda event: event.get("type") == "done" and event.get("id") == 1)
            client.send(type="cancel", id=2)
            client.until(lambda event: event.get("type") == "ack" and event.get("id") == 2)
            frames["stopped"] = self.frame("stopped")
            assert self.drive("state")["panel"]["ask"] is None, "cancelled question still owns the composer"
        finally:
            client.close()
        self.drive("set_composer", {"text": "Continue after cancellation"})
        self.drive("send")
        frames["done-after-cancel"] = self.frame("done", "done-after-cancel")
        assert self.drive("state")["panel"]["ask"] is None

        # Simulate the registry record left by a background await when Desktop
        # restarts. A worker missing from the catalog cannot prove completion.
        pending = self.profile / "runtime/jcode-await-members"
        pending.mkdir(exist_ok=True)
        VERIFY.save(pending / "fixture.json", {
            "session_id": sid, "background": True, "wake": True,
            "requested_ids": ["session_owl_1_fixture"],
            "deadline_unix_ms": time.time_ns() // 1_000_000 + 12_000,
        })
        self.runtime_stop(self.process)
        self.start_desktop(self.test_env, self.test_modes, "-restart")
        assert self.target["session_id"] == sid
        frames["waiting-after-restart"] = self.frame("waiting", "waiting-after-restart")
        if self.visible("swarm-status"):
            panel = self.drive("state")["panel"]
            assert panel["workers_open"] and (panel["workers"] or panel["background_jobs"]["tasks"]), (
                "an unknown worker must not create an empty picker"
            )
            frames["picker"] = self.screenshot("waiting-picker.png")
        self.runtime_wait(lambda: not self.visible("session-status-waiting"), "await deadline")
        frames["after-timeout"] = self.screenshot("after-timeout.png")
        return frames


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--jcode", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    VERIFY.require_safe_host()
    run = args.evidence.resolve()
    run.mkdir(mode=0o700, parents=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="jds-", dir="/tmp") as directory:
            scratch = Path(directory).resolve()
            token = secrets.token_hex(16)
            owner = scratch / ".owner"
            owner.write_text(token)
            owner.chmod(0o600)
            (scratch / "control").mkdir(mode=0o700)
            manifest = {"schema": VERIFY.SCHEMA, "evidence": str(run), "phase": "starting",
                        "scratch": str(scratch), "token": token, "fixture": "swarm-dm",
                        "binary": str(args.desktop.resolve()), "jcode_bin": str(args.jcode.resolve()),
                        "provider": f"http://127.0.0.1:{server.server_port}/v1"}
            fixture = Fixture(run, manifest)
            try:
                fixture.start()
                proof = {"frames": fixture.exercise(), "sources": VERIFY.source_identity(),
                         "desktop_sha256": VERIFY.digest(args.desktop), "jcode_sha256": VERIFY.digest(args.jcode)}
                VERIFY.save(run / "footer-proof.json", proof)
                print(json.dumps(proof, indent=2))
            except Exception:
                if fixture.target:
                    VERIFY.save(run / "failure-state.json", fixture.drive("state"))
                    fixture.screenshot("failure.png")
                raise
            finally:
                VERIFY.save(run / "provider-steps.json", Provider.steps)
                fixture.cleanup("footer acceptance finished")
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    main()
