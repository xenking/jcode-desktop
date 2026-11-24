#!/usr/bin/env python3
"""Drive account routing in hidden macOS Desktop against a private daemon.

A loopback OpenCode Go provider reports per-key usage and records which key each
request used. The daemon runs with `[routing] mode = "auto"`, so a spent key
raises a real spending-limit gate. The script checks the gate card by keyboard
and by mouse, the account chip, `/acct`, and the routing view the sidebar reads, and saves a
screenshot of each. Synthetic data only: nothing real is contacted.

    accept-routing.py --desktop <jcode-desktop> --jcode <jcode> --evidence <dir>
"""
import argparse
import importlib.util
import json
from http.server import ThreadingHTTPServer
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
DAEMON = load(HERE.parent.parent / "jcode/scripts/accept-routing-daemon.py", "routing_daemon")
Provider = DAEMON.Fixture

CONFIG = """[server]
mobile_push = false

[routing]
mode = "auto"

[[routing.accounts]]
id = "opencode-go"
kind = "subscription"
priority = 1
on_limit = "ask"

[[routing.accounts]]
id = "opencode-go:second"
kind = "subscription"
priority = 2
on_limit = "ask"

[[routing.budgets]]
name = "second-5h"
account = "opencode-go:second"
window = "5h"
unit = "percent"
warn = 70
limit = 90
action = "ask"

[[routing.budgets]]
name = "spend-month"
provider = "opencode-go"
meter = "ledger"
window = "calendar-month"
unit = "usd"
warn = 40
limit = 50
action = "ask"

[api_key_profiles.opencode-go]
base_url = "PROVIDER"

[api_key_profiles.opencode-go.keys.second]
key_command = ["echo", "key-second"]
"""


class Fixture(VERIFY.Instance):
    def start_background_runtime(self, env):
        self.runtime_env = {**env, "JCODE_TEST_SESSION": "1"}
        self.legacy_socket = self.profile / "jcode/legacy.sock"
        self.api_socket = self.profile / "jcode/api.sock"
        self.runtime_env.update(
            OPENCODE_GO_API_KEY="key-main", JCODE_NO_AUTO_UPDATE="1", JCODE_TELEMETRY="off",
            JCODE_SOCKET=str(self.legacy_socket), JCODE_API_SOCKET=str(self.api_socket),
        )
        (self.profile / "jcode/config.toml").write_text(CONFIG.replace("PROVIDER", self.manifest["provider"]))
        self.runtime_cli = [self.manifest["jcode_bin"], "--no-update", "--no-selfdev",
                            "--provider", "opencode-go", "--model", "kimi-k2.5"]
        self.daemon = self.runtime_spawn("private-daemon", [*self.runtime_cli, "serve", "--socket", str(self.legacy_socket)])
        self.runtime_wait(self.legacy_socket.exists, "private daemon")
        self.api_bridge = self.runtime_spawn("private-bridge", [
            *self.runtime_cli, "api-bridge", "--api-socket", str(self.api_socket),
        ])
        self.runtime_wait(self.api_socket.exists, "private API bridge")
        session_id = json.loads(self.runtime_debug(f"create_session:{self.profile / 'home'}"))["session_id"]
        env.update(JCODE_TEST_SESSION="1", JCODE_DESKTOP_CONTROL_RUNTIME_SOCKET=str(self.api_socket),
                   JCODE_DESKTOP_CONTROL_RUNTIME_SESSION=session_id)

    def routing(self):
        return self.drive("state")["panel"]["routing"]

    def wait(self, predicate, label, seconds=30):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            time.sleep(.2)
        raise RuntimeError(f"timed out waiting for {label}")

    def submit(self, text):
        self.drive("set_composer", {"text": text})
        self.drive("send")

    def command(self, text):
        """Slash commands go through the composer's own Enter, not the literal control send."""
        self.drive("set_composer", {"text": text})
        self.drive("focus")
        self.drive("press_key", {"key": "fixture:enter"})

    def transcript_text(self):
        return json.dumps(self.drive("state")["panel"].get("loaded_transcript_items") or [])

    def exercise(self):
        frames = {}
        Provider.used.update({"key-main": 99, "key-second": 20})
        Provider.limited = set()
        Provider.seen.clear()
        frames["idle"] = self.screenshot("idle.png")
        assert self.routing()["gate"] is None and self.routing()["chip"] is None

        # A spent key holds the turn behind a gate. Nothing reaches the provider yet.
        self.submit("hello")
        gate = self.wait(lambda: self.routing()["gate"], "the spending-limit gate")
        assert not Provider.seen, "the turn must be held until the person answers"
        rows = gate["rows"]
        assert rows[-1] == "Cancel" and any(row.startswith("Switch to opencode-go:second") for row in rows), rows
        assert gate["cursor"] == next(i for i, row in enumerate(rows) if row.startswith("Switch to")), gate
        frames["gate"] = self.screenshot("gate.png")

        # Keyboard: Down moves the focus, Up returns it, Escape cancels and drops the turn.
        self.drive("press_key", {"key": "fixture:down"})
        self.wait(lambda: self.routing()["gate"]["cursor"] != gate["cursor"] and self.routing()["gate"], "focus to move")
        frames["gate-moved"] = self.screenshot("gate-moved.png")
        self.drive("press_key", {"key": "fixture:up"})
        self.wait(lambda: self.routing()["gate"]["cursor"] == gate["cursor"], "focus to return")
        self.drive("press_key", {"key": "fixture:escape"})
        self.wait(lambda: self.routing()["gate"] is None, "the cancelled gate to close")
        assert not Provider.seen, f"a cancelled turn never reaches the provider: {Provider.seen}"

        # Mouse: nothing changed, so the next turn raises the gate again. One click answers.
        self.submit("again")
        gate = self.wait(lambda: self.routing()["gate"], "the second gate")
        switch = next(i for i, row in enumerate(gate["rows"]) if row.startswith("Switch to opencode-go:second"))
        self.drive("click", {"selector": f"routing-gate-row-{switch}"})
        self.wait(lambda: Provider.seen and set(Provider.seen) == {"key-second"}, "the turn on key-second")
        self.wait(lambda: self.routing()["gate"] is None, "the answered gate to close")
        chip = self.wait(lambda: self.routing()["chip"], "the account chip")
        assert chip["account"] == "opencode-go:second", chip
        frames["chip"] = self.screenshot("chip.png")

        # `/acct status` answers in the transcript, a bad command explains itself.
        self.command("/acct status")
        self.wait(lambda: "routing: auto" in self.transcript_text(), "the /acct status answer")
        self.command("/acct pin")
        self.wait(lambda: "Usage: /acct" in self.transcript_text(), "the /acct usage error")
        frames["acct"] = self.screenshot("acct.png")

        # The routing view the accounts sidebar reads, over the real bridge: accounts with live
        # windows, budgets with used against limit, and the labelled key.
        view = self.harness_requests([("get_routing_view", {})])[0]["view"]
        assert view["mode"] == "auto" and {a["id"] for a in view["accounts"]} == {"opencode-go", "opencode-go:second"}, view
        budgets = {b["name"]: b for b in view["budgets"]}
        assert budgets["second-5h"]["limit"] == 90 and budgets["second-5h"]["unit"] == "percent", budgets
        assert budgets["spend-month"]["unit"] == "usd" and budgets["spend-month"]["limit"] == 50, budgets
        keys = {k["account"]: k for p in view["api_keys"] for k in p["keys"]}
        assert keys["opencode-go:second"]["label"] == "second" and keys["opencode-go:second"]["routed"], keys
        VERIFY.save(self.run / "routing-view.json", view)
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
                        "motion": None,
                        "binary": str(args.desktop.resolve()), "jcode_bin": str(args.jcode.resolve()),
                        "provider": f"http://127.0.0.1:{server.server_port}/v1"}
            fixture = Fixture(run, manifest)
            try:
                fixture.start()
                proof = {"frames": fixture.exercise(), "sources": VERIFY.source_identity(),
                         "desktop_sha256": VERIFY.digest(args.desktop), "jcode_sha256": VERIFY.digest(args.jcode)}
                VERIFY.save(run / "routing-proof.json", proof)
                print(json.dumps(proof, indent=2))
            except Exception:
                if fixture.target:
                    VERIFY.save(run / "failure-state.json", fixture.drive("state"))
                    fixture.screenshot("failure.png")
                raise
            finally:
                VERIFY.save(run / "provider-seen.json", Provider.seen)
                fixture.cleanup("routing acceptance finished")
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    main()
