#!/usr/bin/env python3
"""Own a hidden macOS Desktop verification instance, never the user's app.

The supervisor retains Popen handles. Only it contacts the per-run Desktop socket
or terminates its child. Evidence lives outside the disposable profile.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import time

from screenshot import isolated_env

REPO = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("accept_native_control", REPO / "scripts/accept-native-control.py")
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)
ACTIONS = ("list", "state", "open_session", "set_composer", "send", "press_key", "search", "click", "expand_tool", "wait_for")
SWARM_JOURNEYS = ("swarm-sidebar", "swarm-status")
SWARM_WORKER = "swarm-fixture-0"
SCHEMA = 1


def save(path, value):
    temporary = path.with_suffix(path.suffix + ".new")
    with temporary.open("w", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")
    temporary.chmod(0o600)
    temporary.replace(path)


def digest(path):
    sha = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            sha.update(chunk)
    return sha.hexdigest()


def source_identity():
    """Track dirty build inputs too, not just the commit under test."""
    result = {}
    for root in (REPO, (REPO / "../jcode").resolve()):
        files = subprocess.check_output(["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=root).split(b"\0")
        fingerprint = hashlib.sha256()
        for raw in sorted(set(files)):
            name = os.fsdecode(raw)
            if not name or not (name.endswith((".rs", ".toml", ".lock")) or name.startswith(".cargo/")):
                continue
            path = root / name
            fingerprint.update(raw)
            fingerprint.update(path.read_bytes() if path.is_file() else b"<deleted>")
        result[str(root)] = {
            "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
            "build_inputs_sha256": fingerprint.hexdigest(),
        }
    return result


def require_safe_host():
    if sys.platform != "darwin":
        raise RuntimeError("refusing launch: hidden render-to-image isolation is verified only on macOS; use the existing private-Xvfb acceptance runners on Linux")
    source = (REPO / "src/main.rs").read_text()
    for gate in ('env::var("JCODE_DESKTOP_SCREENSHOT").as_deref() == Ok("1")',
                 'env::var("JCODE_DESKTOP_CONTROL_TEST").as_deref() == Ok("1")',
                 "focus: !control_fixture", "show: !control_fixture", "if !control_fixture {"):
        if gate not in source:
            raise RuntimeError("refusing launch: this checkout lacks the reviewed hidden/non-activating control fixture gate: " + gate)
    if not shutil.which("justrust"):
        raise RuntimeError("justrust is required to build the checkout before launch")


def environment(profile, control):
    env = isolated_env(profile)
    env.update({
        "JCODE_RUNTIME_DIR": str(profile / "runtime"),
        "JCODE_SOCKET": str(profile / "runtime/daemon.sock"),
        "JCODE_API_SOCKET": str(profile / "runtime/api.sock"),
        "JCODE_DESKTOP_CONTROL_DIR": str(control),
        "JCODE_DESKTOP_CONTROL_TEST": "1",
        "JCODE_DESKTOP_MUTE_SOUNDS": "1",
        "JCODE_DESKTOP_WORKING_DIR": str(profile / "home"),
        "JCODE_DESKTOP_CONFIG": str(profile / "desktop.toml"),
        "TMPDIR": str(profile / "tmp"),
        "JCODE_NO_TELEMETRY": "1",
    })
    return env


def seed_swarm_worktrees(profile, env):
    lead = profile / "coordinator"
    worker = profile / "worker"

    def git(*args):
        subprocess.run(["git", *args], env=env, cwd=profile, check=True,
                       capture_output=True, timeout=10)

    git("init", "-b", "main", str(lead))
    git("-C", str(lead), "-c", "user.name=Desktop Fixture", "-c",
        "user.email=fixture@example.invalid", "commit", "--allow-empty", "-qm", "Fixture root")
    git("-C", str(lead), "worktree", "add", "-b", "worker", str(worker))
    return lead, worker


def read_manifest(run):
    path = run / "run.json"
    meta = path.lstat()
    if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
        raise RuntimeError("refusing unsafe run manifest")
    manifest = json.loads(path.read_text())
    if manifest.get("schema") != SCHEMA or manifest.get("evidence") != str(run):
        raise RuntimeError("run manifest does not identify this evidence directory")
    return manifest


def owned_scratch(manifest):
    scratch = Path(manifest["scratch"])
    meta = scratch.lstat()
    if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
        raise RuntimeError("refusing non-private scratch directory")
    marker = scratch / ".owner"
    meta = marker.lstat()
    if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
        raise RuntimeError("refusing unsafe scratch ownership marker")
    if not secrets.compare_digest(marker.read_text(), manifest["token"]):
        raise RuntimeError("refusing scratch not owned by this run")
    return scratch


def rpc(run, command, **fields):
    manifest = read_manifest(run)
    if manifest.get("phase") == "cleaned":
        if command == "cleanup":
            return json.loads((run / "cleanup.json").read_text())
        raise RuntimeError("instance is cleaned; evidence remains, launch a new run")
    endpoint = Path(manifest["supervisor_socket"])
    scratch = owned_scratch(manifest)
    if endpoint != scratch / "supervisor.sock":
        raise RuntimeError("refusing non-private supervisor endpoint")
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(150)
        client.connect(str(endpoint))
        client.sendall(json.dumps({"token": manifest["token"], "command": command, **fields}).encode() + b"\n")
        with client.makefile("rb") as stream:
            reply = json.loads(stream.readline(8 * 1024 * 1024))
    if not reply.get("ok"):
        raise RuntimeError(reply.get("error", "supervisor request failed"))
    return reply["result"]


class Instance:
    def __init__(self, run, manifest):
        self.run = run
        self.manifest = manifest
        self.profile = Path(manifest["scratch"]) / "profile"
        self.control = Path(manifest["scratch"]) / "control"
        self.process = None
        self.endpoint = None
        self.target = None
        self.sequence = 0

    def record(self, request, reply):
        self.sequence += 1
        with (self.run / "actions.jsonl").open("a") as stream:
            stream.write(json.dumps({"step": self.sequence, "request": request, "response": reply}) + "\n")

    def start(self):
        for name in ("home", "runtime", "config", "cache", "data", "logs", "jcode", "tmp"):
            (self.profile / name).mkdir(parents=True, mode=0o700)
        (self.profile / "desktop.toml").write_text('[appearance]\nreduce_motion = true\n[workspace]\ncoaching_hints = false\n')
        env = environment(self.profile, self.control)
        env["JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT"] = self.manifest["fixture"]
        env["JCODE_DESKTOP_SCREENSHOT_SWARM"] = "1"
        modes = ["--single-panel"]
        if self.manifest["fixture"] == "swarm-worktree":
            lead, worker = seed_swarm_worktrees(self.profile, env)
            env.update(JCODE_DESKTOP_WORKING_DIR=str(lead),
                       JCODE_DESKTOP_CONTROL_SWARM_WORKTREE=str(worker),
                       JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="expandable")
            modes = []
        save(self.run / "isolation.json", {"environment": env, "scope": "real hidden GPUI window with offline transcript/session fixtures; no daemon, bridge, provider, model inference or OS input injection"})
        with (self.run / "desktop.log").open("w") as log:
            self.process = subprocess.Popen([self.manifest["binary"], *modes, "--no-hot-reload"], env=env, cwd=self.profile / "home", stdout=log, stderr=log, start_new_session=True)
        self.manifest["desktop_pid"] = self.process.pid
        save(self.run / "run.json", self.manifest)
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"Desktop exited {self.process.returncode}; inspect desktop.log")
            endpoints = list(self.control.glob(f"{self.process.pid}-*.sock"))
            if len(endpoints) == 1:
                self.endpoint = endpoints[0]
                listing = self.native({"action": "list"})
                window = listing["window_id"]
                if not window.startswith(f"{self.process.pid}:"):
                    raise RuntimeError("refusing Desktop endpoint not owned by the child PID")
                panels = [p for row in listing["navigation"]["rows"] for p in row["panels"]]
                if panels:
                    self.target = {"window_id": window, "session_id": panels[0]["session"]}
                    state = self.native({"action": "state", "target": self.target})
                    if state["panel"].get("visible_transcript_items") or self.manifest["fixture"] == "empty":
                        self.manifest.update(phase="ready", target=self.target, desktop_socket=str(self.endpoint))
                        save(self.run / "ready.json", {"target": self.target, "state": state})
                        save(self.run / "run.json", self.manifest)
                        return
            time.sleep(.1)
        raise TimeoutError("private Desktop did not become usable; inspect desktop.log")

    def native(self, action):
        if self.process is None or self.process.poll() is not None:
            raise RuntimeError("owned Desktop process is not alive")
        if self.endpoint is None or self.endpoint.parent != self.control or not self.endpoint.name.startswith(f"{self.process.pid}-"):
            raise RuntimeError("refusing endpoint outside owned Desktop control directory")
        meta = self.endpoint.lstat()
        if not stat.S_ISSOCK(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
            raise RuntimeError("refusing unsafe Desktop socket")
        reply = NATIVE.request(self.endpoint, action)
        self.record(action, reply)
        if reply.get("status") != "ok":
            raise RuntimeError("Desktop rejected request: " + json.dumps(reply))
        return reply["result"]

    def doctor(self):
        if digest(Path(self.manifest["binary"])) != self.manifest["binary_sha256"]:
            raise RuntimeError("binary changed since launch; cleanup and launch a new run")
        if source_identity() != self.manifest["sources"]:
            raise RuntimeError("build inputs changed since launch; cleanup and launch a new run")
        listing = self.native({"action": "list"})
        if listing["window_id"] != self.target["window_id"]:
            raise RuntimeError("Desktop window identity changed")
        state = self.native({"action": "state", "target": self.target})
        if state["panel"]["session_id"] != self.target["session_id"]:
            raise RuntimeError("Desktop session identity changed")
        report = {"healthy": True, "desktop_pid": self.process.pid, "target": self.target,
                  "binary_sha256": self.manifest["binary_sha256"], "version": self.manifest["version"],
                  "build": self.manifest["build"], "scope": "hidden native UI, offline fixtures; provider/auth not exercised"}
        save(self.run / "doctor.json", report)
        return report

    def drive(self, action, params=None, session=None):
        if action not in ACTIONS:
            raise ValueError("unsupported action")
        params = dict(params or {})
        if any(key in params for key in ("v", "action", "target", "path", "destination", "allow_user_window")):
            raise ValueError("routing, permission and output paths are owned by the helper")
        if action == "wait_for" and not 1 <= params.get("timeout_ms", 1000) <= 5000:
            raise ValueError("wait_for timeout_ms must be 1..5000 for the acceptance transport")
        target = dict(self.target)
        if session:
            listing = self.native({"action": "list"})
            known = {p["session"] for row in listing["navigation"]["rows"] for p in row["panels"]}
            if session not in known:
                raise ValueError("session is not open in this private window")
            target["session_id"] = session
        if action == "list":
            return self.native({"action": action})
        if action == "open_session":
            return self.native({"action": action, **params, "destination": {"background_panel": {"window_id": target["window_id"]}}})
        request = {"action": action, **params, "target": target}
        if action in ("set_composer", "send", "press_key", "search", "click"):
            request["allow_user_window"] = True  # Only this supervisor's hidden window.
        return self.native(request)

    def screenshot(self, name, selector=None):
        if not name or Path(name).name != name or not name.endswith(".png"):
            raise ValueError("screenshot name must be a basename ending in .png")
        path = self.run / name
        self.native({"action": "render", "target": self.target, "selector": selector, "path": str(path)})
        data = path.read_bytes()
        if not data.startswith(b"\x89PNG\r\n\x1a\n") or len(data) < 100:
            raise RuntimeError("Desktop did not capture a PNG")
        return {"path": str(path), "sha256": digest(path), "bytes": len(data)}

    def journey(self, name):
        if any((self.run / f"{name}-{suffix}").exists() for suffix in ("before.json", "before.png", "after.json", "after.png", "proof.json")):
            raise RuntimeError("journey evidence already exists; launch a fresh run")
        self.doctor()
        if name in SWARM_JOURNEYS:
            return self.swarm_journey(name)
        # Fix the draft so rotating empty-composer suggestions cannot fake a delta.
        self.drive("set_composer", {"text": "Verification fixture draft"})
        before = self.drive("state")
        save(self.run / f"{name}-before.json", before)
        first = self.screenshot(f"{name}-before.png")
        if name == "tools":
            rows = before["panel"].get("visible_transcript_items") or []
            candidates = [row["selector"] for row in rows if row.get("selector", "").startswith("tool:")]
            if not candidates:
                raise RuntimeError("tools journey requires a visible tool fixture; relaunch with --fixture expandable")
            selector = candidates[0]
            call = selector.removeprefix("tool:")
            self.drive("wait_for", {"condition": {"kind": "selector_visible", "selector": selector}, "timeout_ms": 3000})
            self.drive("click", {"selector": selector})
            expected_regions = set(before["panel"]["expanded_regions"]) ^ {call}
            expected = lambda s: set(s["panel"]["expanded_regions"]) == expected_regions and s["panel"]["composer"] == before["panel"]["composer"]
        elif name == "composer":
            self.drive("set_composer", {"text": "Verification draft, no provider request"})
            expected = lambda s: s["panel"]["composer"] == "Verification draft, no provider request"
        elif name == "search":
            self.drive("search", {"query": "fixture"})
            expected = lambda s: s["search_open"] and s["search"]["query"] == "fixture"
        else:
            raise ValueError("unknown journey")
        deadline = time.monotonic() + 5
        while True:
            after = self.drive("state")
            if expected(after):
                break
            if time.monotonic() > deadline:
                raise RuntimeError("journey did not reach its observable postcondition")
            time.sleep(.05)
        # Allow a production frame after the semantic action before capturing it.
        time.sleep(.2)
        last = self.screenshot(f"{name}-after.png")
        if first["sha256"] == last["sha256"]:
            raise RuntimeError("semantic action produced no screenshot change")
        save(self.run / f"{name}-after.json", after)
        report = {"journey": name, "passed": True, "before": first, "after": last,
                  "scope": "native UI interaction only, not live provider/persistence acceptance"}
        save(self.run / f"{name}-proof.json", report)
        return report

    def swarm_journey(self, name):
        def open_panels(state):
            return [panel for row in state["navigation"]["rows"]
                    for panel in row["panels"] if not panel["closing"]]

        listing = self.drive("list")
        lead_id = self.target["session_id"]
        sessions = {session["session_id"]: session for session in listing["sessions"]}
        lead, worker = sessions.get(lead_id, {}), sessions.get(SWARM_WORKER, {})
        if (self.manifest["fixture"] != "swarm-worktree"
                or worker.get("parent_session_id") != lead_id
                or worker.get("swarm_status") != "working"
                or lead.get("working_dir") != str(self.profile / "coordinator")
                or worker.get("working_dir") != str(self.profile / "worker")):
            raise RuntimeError("journey requires the swarm-worktree fixture")
        if any(panel["session"] == SWARM_WORKER for panel in open_panels(listing)):
            raise RuntimeError("worker must not already be open; launch a fresh run")

        def click(selector):
            self.drive("click", {"selector": selector})

        if name == "swarm-status":
            click("swarm-status")
        before = self.drive("state")
        if name == "swarm-status" and not before["panel"]["workers_open"]:
            raise RuntimeError("status pill did not open the worker list")
        save(self.run / f"{name}-before.json", before)
        first = self.screenshot(f"{name}-before.png")

        click(f"{'sidebar' if name == 'swarm-sidebar' else 'status'}-worker:{SWARM_WORKER}")
        after = self.drive("state")
        workers = [panel for panel in open_panels(after) if panel["session"] == SWARM_WORKER]
        if len(workers) != 1 or not workers[0]["focused"]:
            raise RuntimeError("click did not open and focus exactly one worker panel")
        if name == "swarm-status" and after["panel"]["workers_open"]:
            raise RuntimeError("worker activation did not dismiss the status list")
        save(self.run / f"{name}-after.json", self.drive("state", session=SWARM_WORKER))
        last = self.screenshot(f"{name}-after.png")
        if first["sha256"] == last["sha256"]:
            raise RuntimeError("worker click produced no screenshot change")
        report = {"journey": name, "passed": True, "before": first, "after": last,
                  "scope": "GPUI pointer dispatch; real Git worktrees with offline swarm records"}
        save(self.run / f"{name}-proof.json", report)
        return report

    def cleanup(self, reason):
        scratch = owned_scratch(self.manifest)
        if self.process is not None:
            if self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=5)
            save(self.run / "process-exit.json", {"pid": self.process.pid, "returncode": self.process.returncode})
        if (self.profile / "logs").exists():
            shutil.copytree(self.profile / "logs", self.run / "runtime-logs", dirs_exist_ok=True)
        state = self.profile / "state"
        if state.is_file():
            shutil.copy2(state, self.run / "desktop-state.txt")
        shutil.rmtree(scratch)
        self.manifest["phase"] = "cleaned"
        save(self.run / "run.json", self.manifest)
        report = {"cleaned": True, "reason": reason, "scratch_removed": not scratch.exists(),
                  "desktop_pid": self.process.pid if self.process else None,
                  "evidence": str(self.run), "evidence_files": sorted(p.name for p in self.run.iterdir())}
        save(self.run / "cleanup.json", report)
        return report


def supervise(run):
    manifest = read_manifest(run)
    if manifest["phase"] != "starting":
        raise RuntimeError("refusing duplicate supervisor for an existing run")
    owned_scratch(manifest)
    instance = Instance(run, manifest)
    cleaned = False
    bound = False
    def stop(_sig, _frame):
        raise SystemExit("supervisor terminated")
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    server = socket.socket(socket.AF_UNIX)
    try:
        server.bind(manifest["supervisor_socket"])
        bound = True
        os.chmod(manifest["supervisor_socket"], 0o600)
        server.listen(4)
        server.settimeout(1)
        instance.start()
        deadline = time.monotonic() + manifest["lifetime_seconds"]
        while time.monotonic() < deadline:
            if instance.process.poll() is not None:
                raise RuntimeError("Desktop exited unexpectedly")
            try:
                conn, _ = server.accept()
            except socket.timeout:
                continue
            with conn:
                conn.settimeout(5)
                try:
                    with conn.makefile("rb") as stream:
                        request = json.loads(stream.readline(1024 * 1024))
                    if not secrets.compare_digest(str(request.get("token", "")), manifest["token"]):
                        raise RuntimeError("wrong run token")
                    command = request["command"]
                    if command == "doctor":
                        result = instance.doctor()
                    elif command == "drive":
                        instance.doctor()
                        result = instance.drive(request["action"], request.get("params"), request.get("session"))
                    elif command == "journey":
                        result = instance.journey(request["journey"])
                    elif command == "screenshot":
                        instance.doctor()
                        result = instance.screenshot(request["name"], request.get("selector"))
                    elif command == "cleanup":
                        result = instance.cleanup("requested")
                        cleaned = True
                    else:
                        raise ValueError("unknown supervisor command")
                    reply = {"ok": True, "result": result}
                except Exception as error:
                    reply = {"ok": False, "error": str(error)}
                conn.sendall(json.dumps(reply).encode() + b"\n")
            if cleaned:
                return
        raise TimeoutError("verification lifetime expired")
    except Exception as error:
        save(run / "failure.json", {"error": str(error)})
    finally:
        server.close()
        if bound and not cleaned:
            instance.cleanup("failure or lifetime expiry")


def launch(args):
    require_safe_host()
    run = args.run.resolve()
    run.mkdir(parents=True, mode=0o700, exist_ok=False)
    build_dir = REPO / "target/verify-build"
    binary = build_dir / "debug/jcode-desktop"
    env = dict(os.environ, DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer")
    # Apple's /usr/bin/python3 injects its CommandLineTools SDKROOT at startup.
    # Resolve Xcode's SDK after discarding that inherited value.
    env.pop("SDKROOT", None)
    env["SDKROOT"] = subprocess.check_output(["xcrun", "--sdk", "macosx", "--show-sdk-path"], env=env, text=True).strip()
    command = ["justrust", "build", "-p", "jcode-desktop", "--target-dir", str(build_dir)]
    sources = source_identity()
    try:
        with (run / "build.log").open("w") as log:
            subprocess.run(command, cwd=REPO, env=env, stdout=log, stderr=log, check=True, timeout=1200)
        if sources != source_identity():
            raise RuntimeError("build inputs changed during build; use a fresh run")
        if not binary.is_file():
            raise RuntimeError("justrust did not provide the private verification binary")
    except BaseException as error:
        save(run / "run.json", {"schema": SCHEMA, "phase": "cleaned", "evidence": str(run)})
        save(run / "cleanup.json", {"cleaned": True, "desktop_pid": None, "scratch_removed": True,
                                    "reason": "build failed before app launch", "error": str(error)})
        raise
    scratch = Path(tempfile.mkdtemp(prefix="jdv-", dir="/private/tmp"))
    control = scratch / "control"
    env = environment(scratch / "profile", control)
    try:
        version = subprocess.check_output([str(binary), "--version"], env=env, text=True, timeout=10).strip()
    except BaseException:
        shutil.rmtree(scratch)
        raise
    manifest = {"schema": SCHEMA, "phase": "starting", "evidence": str(run), "scratch": str(scratch),
                "supervisor_socket": str(scratch / "supervisor.sock"), "token": secrets.token_hex(32),
                "binary": str(binary), "binary_sha256": digest(binary), "version": version,
                "sources": sources, "build": command, "fixture": args.fixture,
                "lifetime_seconds": args.lifetime}
    owner = scratch / ".owner"
    owner.write_text(manifest["token"])
    owner.chmod(0o600)
    save(run / "run.json", manifest)
    try:
        with (run / "supervisor.log").open("w") as log:
            proc = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "_supervise", "--run", str(run)], stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        deadline = time.monotonic() + 50
        while time.monotonic() < deadline:
            current = read_manifest(run)
            if current["phase"] == "ready":
                return rpc(run, "doctor")
            if current["phase"] == "cleaned" or proc.poll() is not None:
                raise RuntimeError("isolated launch failed; inspect failure.json, desktop.log and supervisor.log")
            time.sleep(.1)
        raise TimeoutError("isolated launch timed out")
    except BaseException:
        # Terminate only the supervisor we started. Its finally block owns Desktop teardown.
        if "proc" in locals() and proc.poll() is None:
            proc.terminate()
            proc.wait(timeout=15)
        elif not (run / "cleanup.json").exists():
            shutil.rmtree(scratch)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("launch", "doctor", "drive", "screenshot", "cleanup", "_supervise"):
        child = sub.add_parser(command)
        child.add_argument("--run", type=Path, required=True, help="new evidence directory for launch, same directory for later commands")
        if command == "launch":
            child.add_argument("--fixture", choices=("all", "empty", "expandable", "ask", "swarm-worktree"), default="expandable")
            child.add_argument("--lifetime", type=int, choices=range(30, 3601), default=900, metavar="30..3600")
        elif command == "drive":
            group = child.add_mutually_exclusive_group(required=True)
            group.add_argument("--journey", choices=("tools", "composer", "search", *SWARM_JOURNEYS))
            group.add_argument("--action", choices=ACTIONS)
            child.add_argument("--params", default="{}", help="JSON action parameters, routing is supplied by helper")
            child.add_argument("--session", help="another already-open private session ID")
        elif command == "screenshot":
            child.add_argument("--name", required=True, help="new .png basename within evidence directory")
            child.add_argument("--selector", help="visible tool:call_id or row:revision:index, otherwise whole native window")
    args = parser.parse_args()
    try:
        run = args.run.resolve()
        if args.command == "launch":
            result = launch(args)
        elif args.command == "_supervise":
            supervise(run)
            return
        elif args.command == "drive":
            if args.journey:
                result = rpc(run, "journey", journey=args.journey)
            else:
                result = rpc(run, "drive", action=args.action, params=json.loads(args.params), session=args.session)
        elif args.command == "screenshot":
            result = rpc(run, "screenshot", name=args.name, selector=args.selector)
        else:
            result = rpc(run, args.command)
        print(json.dumps(result, indent=2, sort_keys=True))
    except (Exception, KeyboardInterrupt) as error:
        print(f"verify-jcode-desktop: {error}", file=sys.stderr)
        if args.command not in ("launch", "cleanup", "_supervise"):
            print("Run cleanup with the same --run, even after failure. Evidence is retained.", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
