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
import re
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
ACTIONS = ("list", "state", "open_session", "set_composer", "send", "press_key", "search", "click", "expand_tool", "wait_for", "focus", "reveal")
SWARM_JOURNEYS = ("swarm-sidebar", "swarm-status")
ASK_FIXTURES = ("ask", "ask-single", "ask-multi", "ask-rich", "ask-applet", "ask-many", "ask-mcp")
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
        self.runtime_processes = []
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
        reduce_motion = "false" if self.manifest["fixture"] == "orb-todos" else "true"
        theme = self.manifest.get("theme")
        theme_line = f'theme = "{theme}"\n' if theme else ""
        layout_line = 'layout_mode = "normal"\n' if self.manifest["fixture"] in ("openrig-live-split", "gallery-navigation") else ""
        (self.profile / "desktop.toml").write_text(f'[appearance]\n{theme_line}{layout_line}reduce_motion = {reduce_motion}\n[workspace]\ncoaching_hints = false\n')
        env = environment(self.profile, self.control)
        if self.manifest.get("persist_restart_manifest"):
            env["JCODE_DESKTOP_RESTART_DIR"] = str(self.profile / "restart")
        env["JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT"] = self.manifest["fixture"]
        env["JCODE_DESKTOP_SCREENSHOT_SWARM"] = "1"
        if self.manifest.get("window_width"):
            env["JCODE_DESKTOP_CONTROL_WINDOW_WIDTH"] = str(self.manifest["window_width"])
        modes = ["--single-panel"]
        if self.manifest["fixture"] == "files-panel":
            project = self.profile / "sample-project"
            (project / "src").mkdir(parents=True)
            (project / "src" / "main.rs").write_text('fn main() {\n    println!("Hello, Files!");\n}\n')
            (project / "README.md").write_text("# Sample project\n\nA private Files fixture.\n")
            (project / "Cargo.toml").write_text('[package]\nname = "sample-project"\n')
            env.update(JCODE_DESKTOP_WORKING_DIR=str(project),
                       JCODE_DESKTOP_CONTROL_FILES_PANEL="1",
                       JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="expandable")
            modes = []
        if self.manifest["fixture"] == "gallery-navigation":
            env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="gallery-preload",
                       JCODE_DESKTOP_SCREENSHOT_PANELS="2",
                       JCODE_DESKTOP_SCREENSHOT_PANEL_TRANSCRIPTS="gallery-preload,expandable",
                       JCODE_DESKTOP_SCREENSHOT_PANEL_TITLES="Session images,Another session",
                       JCODE_DESKTOP_SCREENSHOT_GALLERY_NAVIGATION="1")
            modes = []
        if self.manifest["fixture"] in ("openrig", "openrig-live", "openrig-live-split"):
            env["JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT"] = "openrig"
            modes = []
        if self.manifest["fixture"] in ("openrig-live", "openrig-live-split"):
            env["JCODE_DESKTOP_CONTROL_OPENRIG_LIVE"] = "1"
        if self.manifest["fixture"] == "openrig-live-split":
            env.update(JCODE_DESKTOP_SCREENSHOT_PANELS="2",
                       JCODE_DESKTOP_SCREENSHOT_PANEL_TRANSCRIPTS="openrig,expandable",
                       JCODE_DESKTOP_SCREENSHOT_PANEL_TITLES="OpenRig,Verification transcript")
        if self.manifest["fixture"] == "cache-card":
            env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="expandable",
                       JCODE_DESKTOP_SCREENSHOT_BAR="high",
                       JCODE_DESKTOP_SCREENSHOT_CACHE_CARD="1")
        if self.manifest["fixture"] == "resume-preview":
            env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="expandable",
                       JCODE_DESKTOP_SCREENSHOT_RESUME_FIXTURE="1")
        if self.manifest["fixture"] == "draft-session":
            (self.profile / "00-sample-project").mkdir()
            pinned = self.profile / "99-pinned-project"
            pinned.mkdir()
            for name in ("footer-alpha", "footer-beta"):
                path = self.profile / "jcode" / "skills" / name / "SKILL.md"
                path.parent.mkdir(parents=True)
                path.write_text(f"---\nname: {name}\ndescription: Verify mode selection\nmode: true\n---\nFixture mode.\n")
            with (self.profile / "desktop.toml").open("a") as config:
                config.write(f"pinned_working_dir = {json.dumps(str(pinned))}\n")
            modes = []
        if self.manifest["fixture"] == "orb-todos":
            modes = []
        if self.manifest["fixture"] == "sidebar-todos":
            session = "session_owl_1795071600000_sidebar"
            todos = self.profile / "jcode" / "todos"
            todos.mkdir()
            save(todos / f"{session}.json", [
                {"id": "review", "content": "Review the migration", "status": "completed",
                 "priority": "high", "stage": 1, "group": "Review"},
                {"id": "check", "content": "Check the isolated build", "status": "in_progress",
                 "priority": "high", "stage": 2, "group": "Verification"},
                {"id": "ship", "content": "Publish the result", "status": "pending",
                 "priority": "medium", "stage": 2, "group": "Verification"},
            ])
            env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="orb-todos",
                       JCODE_DESKTOP_CONTROL_TODO_SESSION=session)
            modes = []
        if self.manifest["fixture"] == "tabbar":
            env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="expandable",
                       JCODE_DESKTOP_SCREENSHOT_PANELS="12",
                       JCODE_DESKTOP_SCREENSHOT_PANEL_TITLES=(
                           "Release checklist,Build and validate,API integration,Desktop navigation,"
                           "Layout review,Runtime diagnostics,Provider configuration,Session recovery,"
                           "Tool output,Workspace settings,Regression tests,Follow-up"))
            if self.manifest.get("fullscreen_layout"):
                env["JCODE_DESKTOP_SCREENSHOT_FULLSCREEN"] = "1"
            modes = ["--no-sidebar"] if self.manifest.get("no_sidebar") else []
        if self.manifest["fixture"] == "tab-statuses":
            env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="expandable",
                       JCODE_DESKTOP_SCREENSHOT_TAB_STATUSES="1",
                       JCODE_DESKTOP_SCREENSHOT_PANELS="5",
                       JCODE_DESKTOP_SCREENSHOT_PANEL_TITLES="Working,Waiting,Question,Done,Idle")
            modes = []
        if self.manifest["fixture"] == "swarm-worktree":
            lead, worker = seed_swarm_worktrees(self.profile, env)
            env.update(JCODE_DESKTOP_WORKING_DIR=str(lead),
                       JCODE_DESKTOP_CONTROL_SWARM_WORKTREE=str(worker),
                       JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="expandable")
            modes = []
        if self.manifest["fixture"] in ("background-jobs", "swarm-dm", "side-question-history"):
            self.start_background_runtime(env)
            modes = []
        if self.manifest["fixture"] in ("swarm-dm", "side-question-history"):
            env.update(JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="empty",
                       JCODE_DESKTOP_SCREENSHOT_HIDE_BETA="1")
        if self.manifest["fixture"] == "side-question-history":
            env["JCODE_DESKTOP_CONTROL_SIDE_QUESTIONS"] = "1"
            self.side_question_desktop_env = env
        if self.manifest.get("queued"):
            env["JCODE_DESKTOP_SCREENSHOT_QUEUED"] = str(self.manifest["queued"])
        if self.manifest.get("queue_edit"):
            env["JCODE_DESKTOP_SCREENSHOT_QUEUE_EDIT"] = "1"
        scope = ("real hidden GPUI window with private daemon, bridge, and loopback provider"
                 if self.runtime_processes else "real hidden GPUI window with offline fixtures; no daemon or provider")
        if env.get("JCODE_DESKTOP_CONTROL_OPENRIG_LIVE") == "1":
            scope = "real hidden GPUI window; Work and Build GETs to OpenRig on loopback only"
        save(self.run / "isolation.json", {"environment": env, "scope": scope})
        self.start_desktop(env, modes)

    def start_desktop(self, env, modes, suffix=""):
        self.endpoint = None
        with (self.run / f"desktop{suffix}.log").open("w") as log:
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
                    if (self.manifest["fixture"] == "compaction"
                            and state["panel"].get("loaded_transcript_items")
                            and not state["panel"].get("visible_transcript_items")):
                        # The async bridge can update after the hidden window's first frame.
                        self.screenshot("compaction-initial.png")
                        state = self.native({"action": "state", "target": self.target})
                    fixture = self.manifest["fixture"]
                    if (state["panel"].get("visible_transcript_items")
                            or fixture in ("empty", "mode-picker", "draft-session", "background-jobs", "swarm-dm", "side-question-history", "voice-rt")
                            or (fixture in ("openrig", "openrig-live", "openrig-live-split")
                                and len(panels) == (2 if fixture == "openrig-live-split" else 1))
                            or (fixture == "tabbar" and state["navigation"]["visible_panels"] == 12)):
                        self.manifest.update(phase="ready", target=self.target, desktop_socket=str(self.endpoint))
                        save(self.run / f"ready{suffix}.json", {"target": self.target, "state": state})
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
                  "build": self.manifest["build"],
                  "scope": ("hidden native UI, private daemon and bridge, loopback provider"
                            if self.runtime_processes else "hidden native UI, offline fixtures; provider/auth not exercised")}
        if self.manifest["fixture"] in ("openrig-live", "openrig-live-split"):
            report["scope"] = "hidden native UI, Work and Build GETs to loopback OpenRig; provider/auth not exercised"
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
            panel = {"window_id": target["window_id"]}
            if "workspace" in params:
                workspace = params.pop("workspace")
                if not isinstance(workspace, int) or isinstance(workspace, bool) or workspace < 1:
                    raise ValueError("workspace must be a one-based integer")
                panel["workspace"] = workspace
            return self.native({"action": action, **params, "destination": {"background_panel": panel}})
        request = {"action": action, **params, "target": target}
        if action in ("set_composer", "send", "press_key", "search", "click", "focus"):
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
        if name == "background-jobs":
            return self.background_jobs_journey()
        if any((self.run / f"{name}-{suffix}").exists() for suffix in ("before.json", "before.png", "after.json", "after.png", "proof.json")):
            raise RuntimeError("journey evidence already exists; launch a fresh run")
        self.doctor()
        if name == "swarm-dm":
            return self.swarm_dm_journey()
        if name in SWARM_JOURNEYS:
            return self.swarm_journey(name)
        if name == "compaction":
            return self.compaction_journey()
        if name == "tab-statuses":
            return self.tab_status_journey()
        if name == "mode-picker":
            return self.mode_picker_journey()
        if name == "composer-footer":
            return self.composer_footer_journey()
        if name == "transcript-scroll":
            return self.transcript_scroll_journey()
        if name == "draft-session":
            return self.draft_session_journey()
        if name == "side-question":
            return self.side_question_journey()
        if name == "side-question-history":
            return self.side_question_history_journey()
        if name == "composer-arrows":
            return self.composer_arrows_journey()
        if name == "composer-advanced":
            return self.composer_advanced_journey()
        if name == "resume-preview":
            return self.resume_preview_journey()
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
        elif name == "background-tasks":
            if self.manifest["fixture"] != name:
                raise RuntimeError("relaunch with --fixture background-tasks")
            selector = "task:rollout-plan"
            if selector in before["panel"]["expanded_regions"]:
                raise RuntimeError("background task must start collapsed")
            self.drive("click", {"selector": selector})
            expected_regions = set(before["panel"]["expanded_regions"]) | {selector}
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

    def composer_arrows_journey(self):
        def key(name):
            self.drive("press_key", {"key": f"fixture:{name}"})

        def composer():
            return self.drive("state")["panel"]["composer"]

        older = "Verification prompt history entry"
        self.drive("set_composer", {"text": older})
        self.drive("send")  # Only the existing private offline fixture.
        draft = "界 👩‍💻 Привет Jcode " * 40 + " THE-END"
        self.drive("set_composer", {"text": draft})
        before = self.screenshot("composer-arrows-before.png")
        key("up")
        if composer() != draft:
            raise RuntimeError("Up replaced a draft from inside its wrapped rows")
        key("backspace")
        edited = composer()
        if not (0 < len(edited) < len(draft) and edited.endswith("THE-END")):
            raise RuntimeError("Backspace after Up did not edit an earlier visual row")
        key("home")
        key("up")
        if composer() != older:
            raise RuntimeError("Up at the top edge did not open history")
        key("down")
        if composer() != edited:
            raise RuntimeError("Down at the history edge did not restore the draft")
        key("ctrl-s")
        if composer():
            raise RuntimeError("Ctrl+S did not clear the stashed draft from the editor")
        stashed = self.screenshot("composer-arrows-stashed.png")
        key("ctrl-s")
        if composer() != edited:
            raise RuntimeError("Ctrl+S did not restore the exact stashed draft")
        after = self.screenshot("composer-arrows-after.png")
        report = {"journey": "composer-arrows", "passed": True, "before": before,
                  "stashed": stashed, "after": after,
                  "scope": "native GPUI key dispatch, no macOS HID injection or provider"}
        save(self.run / "composer-arrows-proof.json", report)
        return report

    def composer_advanced_journey(self):
        if self.manifest["fixture"] != "transcript-scroll":
            raise RuntimeError("relaunch with --fixture transcript-scroll")

        def key(value):
            self.drive("press_key", {"key": f"fixture:{value}"})

        def click(selector):
            self.drive("click", {"selector": selector})

        def draft(text):
            self.drive("set_composer", {"text": text})

        def state(stage, predicate):
            deadline = time.monotonic() + 3
            while True:
                panel = self.drive("state")["panel"]
                if predicate(panel):
                    save(self.run / f"composer-advanced-{stage}.json", panel)
                    return panel
                if time.monotonic() >= deadline:
                    save(self.run / f"composer-advanced-{stage}.json", panel)
                    raise RuntimeError(f"composer proof failed at {stage}")
                time.sleep(.05)

        literal = "вывывы /skills"
        draft(literal)
        state("menu", lambda p: p["composer_fixture"]["command_menu"])
        self.screenshot("composer-advanced-menu.png")
        key("up")
        key("escape")
        state("escaped", lambda p: p["composer"] == literal and not p["composer_fixture"]["command_menu"])
        key("enter")
        short_height = state("literal", lambda p: not p["composer"])["composer_fixture"]["editor_height"]

        try:
            click("composer:images")
        except RuntimeError as error:
            if "selector_not_visible" not in str(error):
                raise
        else:
            raise RuntimeError("image toggle appeared without any images")
        draft("image draft")
        key("seed-composer-image")
        with_image = state("attached", lambda p: p["composer_fixture"]["attachments"] == 1)["composer"]
        click("composer:images")
        self.screenshot("composer-advanced-gallery.png")
        click("composer:images")
        key("up")
        state("history", lambda p: p["composer"] == literal and p["composer_fixture"]["attachments"] == 0)
        key("down")
        state("restored", lambda p: p["composer"] == with_image and p["composer_fixture"]["attachments"] == 1)
        key("escape")

        draft("Long draft line\n" * 100)
        automatic = state("automatic", lambda p: p["composer_fixture"]["editor_height"] > short_height)["composer_fixture"]["editor_height"]
        key("alt-shift-up")
        state("keyboard", lambda p: p["composer_fixture"]["editor_height"] > automatic)
        key("alt-shift-down")
        click("composer:resize-drag")
        state("dragged", lambda p: p["composer_fixture"]["editor_height"] > automatic)
        self.screenshot("composer-advanced-expanded.png")
        click("composer:resize-reset")
        state("reset", lambda p: p["composer_fixture"]["editor_height"] == automatic)

        click("history:scroll-up")
        click("composer:wheel-up")
        before = state("before-wheel", lambda p: p["composer_fixture"]["editor_y"] == 0)["scroll_fixture"]["transcript"]
        click("composer:wheel-down")
        state("editor-wheel", lambda p: p["composer_fixture"]["editor_y"] < 0 and p["scroll_fixture"]["transcript"] == before)
        click("composer:wheel-down")
        state("editor-edge", lambda p: p["scroll_fixture"]["transcript"] == before)
        report = {"journey": "composer-advanced", "passed": True,
                  "scope": "hidden native GPUI dispatch, fixture image seed; no HID or provider"}
        save(self.run / "composer-advanced-proof.json", report)
        return report

    def resume_preview_journey(self):
        if self.manifest["fixture"] != "resume-preview":
            raise RuntimeError("relaunch with --fixture resume-preview")
        beta = "resume-fixture-beta"
        alpha = "resume-fixture-alpha"

        def key(value):
            self.drive("press_key", {"key": f"fixture:{value}"})

        def click(selector):
            self.drive("click", {"selector": selector})

        def expect(stage, predicate, session=None):
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                state = self.drive("state", session=session)
                if predicate(state):
                    save(self.run / f"resume-preview-{stage}.json", state)
                    return state
                time.sleep(.05)
            raise RuntimeError(f"resume-preview did not reach {stage}")

        def view(state):
            return (state.get("search") or {}).get("fixture_preview")

        def selected(state, sid):
            return (state.get("search") or {}).get("selected") == sid and bool(view(state))

        def open_panels(state):
            return [p for row in state["navigation"]["rows"] for p in row["panels"] if not p.get("closing")]

        def opened(state, sid):
            return not state["search_open"] and any(
                p["focused"] and p["session"] == sid for p in open_panels(state)
            )

        def anchor(rows):
            return "Beta search anchor" in json.dumps(rows)

        self.drive("search", {"query": "Resume acceptance"})
        expect("before", lambda s: selected(s, alpha))
        before = self.screenshot("resume-preview-before.png")
        click(f"resume-row:{beta}")
        row = expect("selected", lambda s: selected(s, beta))
        picked = self.screenshot("resume-preview-selected.png")
        key("tab")
        expect("focused", lambda s: selected(s, beta) and s["search"]["preview_focus"])
        key("pageup")
        scrolled = expect("keyboard", lambda s: selected(s, beta) and view(s) != view(row))
        click("resume-preview:wheel")
        expect("wheel", lambda s: selected(s, beta) and view(s) != view(scrolled))
        key("escape")
        expect("cancelled", lambda s: opened(s, self.target["session_id"]))

        self.drive("search", {"query": "Beta"})
        expect("match", lambda s: selected(s, beta) and anchor(view(s)))
        match = self.screenshot("resume-preview-match.png")
        click(f"resume-action:{beta}")
        opened_beta = expect("opened", lambda s: opened(s, beta) and anchor(s["panel"]["visible_transcript_items"]), beta)
        after = self.screenshot("resume-preview-after.png")

        self.drive("search", {"query": "Alpha"}, session=beta)
        key("enter")
        expect("alpha", lambda s: opened(s, alpha))
        self.drive("search", {"query": "Beta"}, session=alpha)
        click(f"resume-action:{beta}")
        focused = expect("go-to", lambda s: opened(s, beta), beta)
        if focused["panel_id"] != opened_beta["panel_id"] or sum(
            p["session"] == beta for p in open_panels(focused)
        ) != 1:
            raise RuntimeError("Go to duplicated the existing Beta panel")
        if len({shot["sha256"] for shot in (before, picked, match, after)}) != 4:
            raise RuntimeError("resume-preview did not render distinct interaction stages")
        report = {"passed": True, "scope": "native GPUI pointer, wheel and keyboard dispatch with offline history",
                  "screenshots": [before, picked, match, after]}
        save(self.run / "resume-preview-proof.json", report)
        return report

    def side_question_journey(self):
        if self.manifest["fixture"] != "draft-session":
            raise RuntimeError("side-question requires --fixture draft-session")
        source = "session_verifier_draft"
        question = "What is the retry policy?"
        self.drive("set_composer", {"text": "Start a background wait"})
        self.drive("click", {"selector": "composer.send"})
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            listing = self.drive("list")
            if any(p["session"] == source for row in listing["navigation"]["rows"] for p in row["panels"]):
                self.target["session_id"] = source
                before = self.drive("state")
                if before["panel"]["status"] == "running_tools":
                    break
            time.sleep(.05)
        else:
            raise RuntimeError("the fixture's first prompt did not start running tools")
        save(self.run / "side-question-before.json", before)
        self.screenshot("side-question-before.png")
        self.drive("set_composer", {"text": "/btw"})
        self.drive("press_key", {"key": "fixture:enter"})
        self.runtime_wait(lambda: self.drive("state")["panel"]["fixture_side_questions"]["history"] == [],
                          "bare /btw to show empty history")
        self.screenshot("side-question-empty.png")
        self.drive("set_composer", {"text": f"/btw {question}"})
        self.drive("press_key", {"key": "fixture:enter"})
        expected = {"question": question, "answer": "The retry policy uses a short backoff.", "working": False}
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            after = self.drive("state")
            if after["panel"]["fixture_side_question"] == expected:
                break
            time.sleep(.05)
        else:
            raise RuntimeError("the hidden fork's answer did not reach the card")
        if (after["panel"]["status"] != "running_tools" or after["panel"]["composer"]
                or after["panel"]["loaded_transcript_items"] != before["panel"]["loaded_transcript_items"]
                or after["fixture_sidebar_sessions"] != [source]):
            raise RuntimeError("the side question changed the parent chat or leaked into the sidebar")
        save(self.run / "side-question-after.json", after)
        photo = self.screenshot("side-question-after.png")
        proof = {"passed": True, "scope": "native Enter with an offline side question reply",
                 "source_status": after["panel"]["status"], "card": expected, "screenshot": photo}
        save(self.run / "side-question-proof.json", proof)
        return proof

    def side_question_history_journey(self):
        if self.manifest["fixture"] != "side-question-history":
            raise RuntimeError("side-question-history requires the private runtime fixture")
        parent = self.target["session_id"]
        ordinary = "btw this is an ordinary message"
        first, follow, second = "Topic A verification", "Follow up A verification", "Topic B verification"
        follow_promoted = "Continue the promoted topic A"
        child_model, child_effort = "gpt-5-btw-child", "high"
        draft = "Keep this main draft unsent"
        provider_log = self.run / "fake-requests.jsonl"

        def requests():
            return [json.loads(line) for line in provider_log.read_text().splitlines()] if provider_log.exists() else []

        def user_prompts(request):
            return [re.sub(r"^\[[^\]\n]+\] ", "", message["content"])
                    for message in request["messages"] if message["role"] == "user"
                    and not message["content"].startswith("<system-reminder>")]

        def panel():
            return self.drive("state")["panel"]

        def transcript_text(items):
            return [item[key] for item in items if isinstance(item, dict)
                    for key in ("User", "Assistant") if key in item]

        def side():
            return panel()["fixture_side_questions"]

        def enter():
            self.drive("press_key", {"key": "fixture:enter"})

        def click(selector):
            self.drive("click", {"selector": selector})

        def submit(text):
            self.drive("set_composer", {"text": text})
            enter()

        def answered(last_question):
            def check():
                state = side()
                card = state["card"]
                return (card if state["history"] is None and card and not card["working"]
                        and card["turns"][-1]["question"] == last_question
                        and card["turns"][-1]["answer"] else None)
            return self.runtime_wait(check, f"saved answer for {last_question}")

        self.runtime_wait(lambda: panel()["model"], "visible parent runtime")
        submit(ordinary)
        self.runtime_wait(lambda: len(requests()) == 1 and panel()["status"] == "idle", "ordinary parent message")
        self.drive("set_composer", {"text": "btw"})
        self.drive("send")
        baseline = self.runtime_wait(lambda: p if (p := panel())["status"] == "idle"
                                     and len(transcript_text(p["loaded_transcript_items"])) == 4 else None,
                                     "Desktop control sent literal btw to the parent")
        submit("/btw")
        self.runtime_wait(lambda: side()["history"] == [], "empty parent history")
        assert len(requests()) == 2, "bare /btw contacted the provider"
        submit(f"/btw {first}")
        topic_a = answered(first)
        child_a = topic_a["session_id"]
        _, _, _, info = self.harness_requests([
            ("attach_session", {"session_id": child_a}),
            ("set_model", {"session_id": child_a, "model": child_model}),
            ("set_reasoning_effort", {"session_id": child_a, "effort": child_effort}),
            ("get_runtime_info", {"session_id": child_a}),
        ])
        assert (info["model"], info["reasoning_effort"]) == (child_model, child_effort)
        self.drive("set_composer", {"text": draft})
        click("side-question-follow-up")
        self.drive("press_key", {"key": f"fixture:follow-up-text:{follow}"})
        enter()
        enter()
        continued = answered(follow)
        assert continued["session_id"] == topic_a["session_id"]
        assert [turn["question"] for turn in continued["turns"]] == [first, follow]
        assert panel()["composer"] == draft
        assert panel()["loaded_transcript_items"] == baseline["loaded_transcript_items"]
        click("side-question-dismiss")
        assert panel()["composer_focus"], "dismiss did not return focus to the main composer"
        submit(f"/btw {second}")
        topic_b = answered(second)
        assert topic_b["session_id"] != topic_a["session_id"], "new topic reused the old child"
        click("side-question-dismiss")

        self.runtime_stop(self.api_bridge)
        self.runtime_stop(self.daemon)
        self.api_socket.unlink(missing_ok=True)
        self.legacy_socket.unlink(missing_ok=True)
        self.start_background_daemon()
        self.runtime_stop(self.process)
        self.start_desktop(self.side_question_desktop_env, [], suffix="-restarted")
        assert self.target["session_id"] == parent
        self.runtime_wait(lambda: panel()["model"], "parent reattached after Desktop restart")
        submit("btw")
        topics = self.runtime_wait(lambda: h if isinstance(h := side()["history"], list) and len(h) == 2 else None,
                                   "two persisted side questions after daemon and bridge restart")
        assert {item["session_id"] for item in topics} == {topic_a["session_id"], topic_b["session_id"]}
        assert {item["question"] for item in topics} == {first, second}
        self.drive("set_composer", {"text": draft})
        history_frame = self.screenshot("side-question-history-before.png")
        index = next(i for i, item in enumerate(topics) if item["session_id"] == topic_a["session_id"])
        click(f"side-question-topic-{index}")
        reopened = answered(follow)
        assert reopened["turns"] == continued["turns"] and reopened["session_id"] == child_a
        assert (reopened["model"], reopened["reasoning_effort"]) == (child_model, child_effort)
        after = self.drive("state")
        assert after["panel"]["composer"] == draft
        assert transcript_text(after["panel"]["loaded_transcript_items"]) == transcript_text(baseline["loaded_transcript_items"])
        assert [p["session"] for row in after["navigation"]["rows"] for p in row["panels"]] == [parent]
        child_ids = {child_a, topic_b["session_id"]}
        assert child_ids.isdisjoint(after["fixture_sidebar_sessions"]), "private topics leaked into the sidebar"
        assert child_ids.isdisjoint(worker["session_id"] for worker in after["panel"]["workers"]), "side topics masqueraded as swarm workers"
        proof = {"passed": True, "parent": parent, "topics": topics, "continued": reopened,
                 "history": history_frame, "answer": self.screenshot("side-question-history-after.png")}
        click("side-question-promote")
        try:
            click("side-question-promote")
        except RuntimeError as error:
            if '"code": "selector_not_visible"' not in str(error):
                raise
        else:
            raise AssertionError("the opened topic still accepts a second promotion")
        promoted = reopened["session_id"]
        visibility = self.profile / "jcode/state/side-question-visibility" / f"{promoted}.json"
        self.runtime_wait(lambda: visibility.read_text() == "false", "daemon persisted native promotion")
        navigation = self.drive("list")["navigation"]
        assert [p["session"] for row in navigation["rows"] for p in row["panels"]].count(promoted) == 1

        def sidebar_after_promotion():
            current = self.drive("state")
            return current if promoted in current["fixture_sidebar_sessions"] else None

        promoted_state = self.runtime_wait(sidebar_after_promotion, "promoted chat appeared in the sidebar")
        assert promoted_state["fixture_sidebar_sessions"].count(promoted) == 1
        assert topic_b["session_id"] not in promoted_state["fixture_sidebar_sessions"]
        assert child_ids.isdisjoint(worker["session_id"] for worker in promoted_state["panel"]["workers"])
        assert panel()["composer"] == draft
        assert len(requests()) == 5, "history or promotion started a request"
        self.target["session_id"] = promoted
        self.runtime_wait(lambda: (panel()["model"], panel()["reasoning_effort"]) == (child_model, child_effort),
                          "promoted chat retained the restored child model and effort")
        submit(follow_promoted)
        self.runtime_wait(lambda: len(requests()) == 6 and panel()["status"] == "idle", "one promoted follow-up")
        assert visibility.read_text() == "false", "follow-up hid the promoted chat"
        assert self.drive("state", session=parent)["panel"]["composer"] == draft
        wire = requests()
        assert [user_prompts(request)[-1] for request in wire] == [ordinary, "btw", first, follow, second, follow_promoted]
        assert user_prompts(wire[3]).count(first) == 1, "follow-up duplicated the original side question"
        assert topic_a["turns"][0]["answer"] in json.dumps(wire[3]["messages"])
        assert first not in user_prompts(wire[4]), "new topic inherited side question A"
        assert (wire[-1]["model"], wire[-1]["reasoning_effort"]) == (child_model, child_effort)
        save(self.run / "side-question-history-after.json", after)
        proof.update(model_requests=len(wire), promoted=promoted)
        save(self.run / "side-question-history-proof.json", proof)
        return proof

    def draft_session_journey(self):
        if self.manifest["fixture"] != "draft-session":
            raise RuntimeError("draft-session journey requires --fixture draft-session")
        project = self.profile / "00-sample-project"
        before = self.drive("state")
        if (before["panel"]["model"] != "gpt-5.6-terra"
                or before["fixture_sidebar_sessions"]):
            raise RuntimeError("fresh draft did not expose the default model or leaked into the sidebar")
        save(self.run / "draft-session-before.json", before)
        self.screenshot("draft-session-before.png")
        for selector in ("composer-location", "folder-picker-entry-0", "folder-picker-open",
                         "panel-model", "draft-model-row:gpt-5.6-sol", "panel-model-effort",
                         "draft-effort-high"):
            self.drive("click", {"selector": selector})
        configured = self.drive("state")
        if (configured["panel_id"] != before["panel_id"]
                or configured["panel"]["working_dir"] != str(project)
                or configured["panel"]["model"].split(":")[-1] != "gpt-5.6-sol"
                or configured["panel"]["reasoning_effort"] != "high"
                or configured["fixture_sidebar_sessions"]):
            raise RuntimeError(f"configuring the empty draft changed its panel, leaked it, or lost settings: {configured}")
        save(self.run / "draft-session-configured.json", configured)
        self.screenshot("draft-session-configured.png")
        self.drive("set_composer", {"text": "First native draft prompt"})
        self.drive("click", {"selector": "composer.send"})
        sid = "session_verifier_draft"
        deadline = time.monotonic() + 5
        while True:
            listing = self.drive("list")
            if any(p["session"] == sid for row in listing["navigation"]["rows"] for p in row["panels"]):
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("first prompt did not promote the draft")
            time.sleep(.05)
        self.target["session_id"] = sid
        photo = self.screenshot("draft-session-after.png")
        after = self.drive("state")
        if (after["panel_id"] != before["panel_id"] or after["fixture_sidebar_sessions"] != [sid]
                or after["panel"]["working_dir"] != str(project)):
            raise RuntimeError(f"promoted draft not in the expected panel and sidebar: {after}")
        save(self.run / "draft-session-after.json", after)
        self.drive("click", {"selector": "tab-new-session"})
        listing = self.drive("list")
        draft_ids = [p["session"] for row in listing["navigation"]["rows"] for p in row["panels"]
                     if p["session"].startswith("startup://draft/")]
        if len(draft_ids) != 1:
            raise RuntimeError(f"tab plus did not create one new draft: {draft_ids}")
        self.target["session_id"] = draft_ids[0]
        new_draft = self.drive("state")
        pinned = self.profile / "99-pinned-project"
        if (new_draft["panel"]["working_dir"] != str(pinned)
                or new_draft["panel"]["model"] != configured["panel"]["model"]
                or new_draft["panel"]["reasoning_effort"] != configured["panel"]["reasoning_effort"]):
            raise RuntimeError(f"tab plus lost the preferences on the new draft: {new_draft}")
        save(self.run / "draft-session-next.json", new_draft)
        report = {"passed": True, "scope": "native UI with offline catalog and create reply",
                  "working_dir": str(project), "new_draft_directory": str(pinned), "model": configured["panel"]["model"],
                  "effort": configured["panel"]["reasoning_effort"], "sidebar": [sid], "screenshot": photo}
        save(self.run / "draft-session-proof.json", report)
        return report

    def mode_picker_journey(self):
        if self.manifest["fixture"] != "mode-picker":
            raise RuntimeError("journey requires the mode-picker fixture")
        name = "mode-picker"
        expected = ["selfdev", "swarm", "poteto-mode"]
        frames = {}
        for stage, text in [("before", "Verification draft"), ("after", "/mode"),
                            ("filtered", "/mode quiet"), ("closed", "Verification draft")]:
            self.drive("set_composer", {"text": text})
            state = self.drive("state")
            if state["panel"]["composer"] != text or state["panel"]["active_modes"] != expected:
                raise RuntimeError(f"mode-picker fixture or composer mismatch at {stage}")
            rows = state["panel"]["mode_rows"]
            names = {"before": [], "after": [*expected, "quiet", "verify-jcode", "write-guide", "off"],
                     "filtered": ["quiet"], "closed": []}
            if [row["name"] for row in rows] != names[stage]:
                raise RuntimeError(f"mode-picker rows mismatch at {stage}")
            if stage == "after" and ([row["name"] for row in rows if row["active"]] != expected
                                     or [row["section"] for row in rows if row["section"]] != ["Active", "Modes", "Skills", "Actions"]
                                     or [row["icon"] for row in rows] != ["selfdev", "swarm", "crown", "moon", "todo", "pencil", "trash"]):
                raise RuntimeError("mode-picker active marks, sections or icons mismatch")
            save(self.run / f"{name}-{stage}.json", state)
            frames[stage] = self.screenshot(f"{name}-{stage}.png")
        hashes = {stage: frame["sha256"] for stage, frame in frames.items()}
        if len({hashes[stage] for stage in ("before", "after", "filtered")}) != 3 or hashes["before"] != hashes["closed"]:
            raise RuntimeError("mode picker did not visually open, filter and close")
        report = {"journey": name, "passed": True, "active_modes": expected, "frames": frames,
                  "scope": "native menu and chips with seeded runtime response; no backend mutations"}
        save(self.run / f"{name}-proof.json", report)
        return report

    def composer_footer_journey(self):
        if self.manifest["fixture"] != "draft-session":
            raise RuntimeError("composer-footer journey requires --fixture draft-session")

        def key(value):
            self.drive("press_key", {"key": f"fixture:{value}"})

        def click(selector):
            self.drive("click", {"selector": selector})

        def panel():
            return self.drive("state")["panel"]

        draft = "Keep this draft"
        self.drive("set_composer", {"text": draft})
        before = self.screenshot("composer-footer-before.png")
        click("panel-mode")
        modes = ["footer-alpha", "footer-beta"]
        if [row["name"] for row in panel()["mode_rows"]][:2] != modes:
            raise RuntimeError("mode button did not open the two fixture modes")
        for navigation in ("tab", "down", "tab"):
            key(navigation)
        opened = panel()
        if opened["active_modes"] != modes or not all(row["active"] for row in opened["mode_rows"][:2]):
            raise RuntimeError("Tab did not toggle and retain both modes")
        key("enter")
        if panel()["mode_rows"]:
            raise RuntimeError("Enter did not close the mode picker")
        key("space")
        if not panel()["mode_rows"]:
            raise RuntimeError("the mode trigger did not regain keyboard focus")
        key("escape")
        click("panel-model")
        click("draft-model-row:gpt-5.6-sol")
        click("panel-model-effort")
        click("draft-effort-high")
        configured = panel()
        if (configured["composer"] != draft or configured["active_modes"] != modes
                or configured["model"].split(":")[-1] != "gpt-5.6-sol"
                or configured["reasoning_effort"] != "high"):
            raise RuntimeError("footer controls lost the draft, modes, model, or effort")
        selected = self.screenshot("composer-footer-selected.png")

        self.drive("set_composer", {"text": "/effort"})
        slash = self.screenshot("composer-footer-slash-effort.png")
        for _ in range(3):
            key("down")
        key("enter")
        after = panel()
        if (after["composer"] or after["reasoning_effort"] != "medium"
                or after["model"] != configured["model"] or after["active_modes"] != modes):
            raise RuntimeError("standalone /effort did not change only effort with Down and Enter")
        report = {"passed": True, "before": before, "selected": selected, "slash_effort": slash,
                  "model": after["model"], "effort": after["reasoning_effort"], "modes": modes,
                  "scope": "hidden macOS GPUI mouse and keyboard dispatch, offline draft"}
        save(self.run / "composer-footer-proof.json", report)
        return report

    def transcript_scroll_journey(self):
        if self.manifest["fixture"] != "transcript-scroll":
            raise RuntimeError("journey requires the transcript-scroll fixture")

        def state():
            return self.drive("state")["panel"]["scroll_fixture"]

        def click(selector, expected, failure, frame=None):
            self.drive("click", {"selector": selector})
            # The private window is hidden and can lack a presentation tick.
            # Rendering forces layout before reading the measured geometry.
            shot = self.screenshot(frame or f"transcript-scroll-{selector.replace(':', '-')}-frame.png")
            deadline = time.monotonic() + 3
            while True:
                measured = state()
                if expected(measured):
                    return measured, shot
                if time.monotonic() >= deadline:
                    raise RuntimeError(f"{failure}: {measured}")
                time.sleep(.05)

        self.drive("set_composer", {"text": "Verification fixture draft"})
        first = self.screenshot("transcript-scroll-before.png")
        inline = state()
        if (inline["prompt_height"] or 0) <= 2 * inline["viewport_height"] or inline["pinned_prompt"] is not None:
            raise RuntimeError(f"initial prompt is not fully expanded in the transcript: {inline}")
        opened, _ = click("tool:scroll-fixture", lambda s: s["tool"] and s["tool"][1] > 0,
                       "expanded tool has no scroll range")
        at_edge, _ = click("scroll:tool", lambda s: s["tool"][0] == -s["tool"][1],
                        "tool did not scroll to its edge")
        offset, maximum = at_edge["tool"]
        if maximum <= 0 or offset != -maximum or at_edge["transcript"] != opened["transcript"]:
            raise RuntimeError("the first gesture did not scroll only the nested tool to its edge")
        self.drive("click", {"selector": "scroll:momentum"})
        last = self.screenshot("transcript-scroll-after.png")
        if state() != at_edge:
            raise RuntimeError("momentum at the tool edge changed scroll positions")
        outside, _ = click("scroll:transcript", lambda s: s["transcript"] != at_edge["transcript"],
                        "a new gesture outside the tool did not scroll the transcript")
        compact, pinned = click("scroll:pin-prompt", lambda s: s["pinned_prompt"] == 1,
                                "scrolling did not pin the prompt", "transcript-scroll-pinned.png")
        full, expanded = click("prompt:toggle", lambda s: s["pinned_prompt_expanded"] == 1,
                               "pinned prompt did not expand", "transcript-scroll-expanded.png")
        closed, _ = click("prompt:toggle", lambda s: s["pinned_prompt_expanded"] is None,
                          "pinned prompt did not collapse", "transcript-scroll-collapsed.png")
        for measured in (full, closed):
            if any(measured[key] != compact[key] for key in ("transcript", "viewport_y", "viewport_height", "prompt_height")):
                raise RuntimeError(f"pinned disclosure changed the transcript anchor or layout: {measured}")
        proof = {"passed": True, "before": first, "pinned": pinned, "expanded": expanded, "after": last,
                 "inline": inline, "compact": compact, "full": full, "at_edge": at_edge, "outside": outside}
        save(self.run / "transcript-scroll-proof.json", proof)
        return proof

    def tab_status_journey(self):
        if self.manifest["fixture"] != "tab-statuses":
            raise RuntimeError("journey requires the tab-statuses fixture")
        listing = self.drive("list")
        sessions = [panel["session"] for row in listing["navigation"]["rows"]
                    for panel in row["panels"] if not panel["closing"]]
        panels = [self.drive("state", session=session)["panel"] for session in sessions]
        expected = ["working", "waiting", "question", "done", None]
        if [panel["tab_activity"] for panel in panels] != expected:
            raise RuntimeError("tab activities did not match the fixture states")
        first = self.screenshot("tab-statuses-before.png")
        question = panels[2]["ask"]
        self.drive("click", {"selector": question["option_selectors"][0]}, session=sessions[2])
        after = self.drive("state", session=sessions[2])["panel"]
        if after["tab_activity"] != "question" or after["ask"]["chosen"] == question["chosen"]:
            raise RuntimeError("choosing a response did not preserve the unanswered question frame")
        time.sleep(.2)
        last = self.screenshot("tab-statuses-after.png")
        if first["sha256"] == last["sha256"]:
            raise RuntimeError("choosing a response produced no screenshot change")
        report = {"journey": "tab-statuses", "passed": True, "states": expected,
                  "before": first, "after": last, "scope": "native UI on offline fixture"}
        save(self.run / "tab-statuses-proof.json", report)
        return report

    def compaction_journey(self):
        if self.manifest["fixture"] != "compaction":
            raise RuntimeError("journey requires the compaction fixture")
        before = self.drive("state")
        initial = before["panel"]["loaded_transcript_items"]
        oldest = {"User": "Before compaction 000"}
        after_compaction = {"User": "After compaction"}
        divider = next(row["Compaction"] for row in initial if "Compaction" in row)
        if oldest in initial or {"User": "Before compaction 099"} not in initial:
            raise RuntimeError("the initial transcript must show only a tail of the archived messages")
        if divider["state"] != "Collapsed":
            raise RuntimeError("the compaction divider did not start collapsed")
        self.screenshot("compaction-collapsed.png")
        self.drive("click", {"selector": "compaction-divider"})
        self.screenshot("compaction-expanded.png")
        expanded = self.drive("state")["panel"]["loaded_transcript_items"]
        divider = next(row["Compaction"] for row in expanded if "Compaction" in row)
        label, _, detail = divider["content"].partition("\n\n")
        if (divider["state"] != "Expanded" or label != "compacted · snapcompact · 524k→100k"
                or "Scroll up to read earlier messages." not in detail
                or any(text in detail for text in ("Resume prior conversation", "Archived transcript scopes", "HISTORY"))):
            raise RuntimeError(f"unexpected expanded divider: {divider}")

        deadline = time.monotonic() + 5
        while True:
            self.drive("click", {"selector": "history:scroll-up"})
            time.sleep(.1)
            after = self.drive("state")
            items = after["panel"]["loaded_transcript_items"]
            if oldest in items:
                break
            if time.monotonic() > deadline:
                raise RuntimeError("scrolling up did not trigger a compacted history page")
        if items.count(oldest) != 1 or items[items.index(oldest) + 1:] != [
            *({"User": f"Before compaction {i:03}"} for i in range(1, 100)),
            {"Compaction": divider}, after_compaction,
        ]:
            raise RuntimeError("the older page duplicated, reordered, or replaced transcript rows")
        save(self.run / "compaction-before.json", before)
        save(self.run / "compaction-after.json", after)
        proof = {"older_rows_added": len(items) - len(initial), "expanded_divider": divider}
        save(self.run / "compaction-proof.json", proof)
        return proof

    def start_background_runtime(self, env):
        self.runtime_env = {**env, "JCODE_TEST_SESSION": "1"}
        runtime = self.runtime_env
        self.legacy_socket = self.profile / "jcode/legacy.sock"
        self.api_socket = self.profile / "jcode/api.sock"
        runtime.update(JCODE_PROVIDER="openai-compatible", JCODE_MODEL="fake-model",
                       OPENAI_COMPAT_API_KEY="fixture-only", JCODE_OPENAI_COMPAT_DEFAULT_MODEL="fake-model",
                       FAKE_LOG=str(self.run / "fake-requests.jsonl"),
                       FAKE_CONTROL_DIR=str(self.profile), FAKE_PORT_FILE=str(self.profile / "fake-port"),
                       JCODE_SOCKET=str(self.legacy_socket), JCODE_API_SOCKET=str(self.api_socket))
        config = "[server]\nmobile_push = false\n"
        if self.manifest["fixture"] == "side-question-history":
            config += "[features]\nmemory = false\n"
            runtime["FAKE_PREMIUM_MODEL"] = "gpt-5-btw-child"
        (self.profile / "jcode/config.toml").write_text(config)
        self.runtime_cli = [self.manifest["jcode_bin"], "--no-update", "--no-selfdev"]
        provider = REPO.parent / "jcode/scripts/fake_openai_server.py"
        self.runtime_spawn("fake-provider", [sys.executable, str(provider), "0"])
        port_file = Path(runtime["FAKE_PORT_FILE"])
        self.runtime_wait(port_file.is_file, "loopback provider")
        runtime["JCODE_OPENAI_COMPAT_API_BASE"] = f"http://127.0.0.1:{port_file.read_text().strip()}/v1"
        self.start_background_daemon()
        session_id = json.loads(self.runtime_debug(f"create_session:{self.profile / 'home'}"))["session_id"]
        env.update(JCODE_TEST_SESSION="1", JCODE_DESKTOP_CONTROL_RUNTIME_SOCKET=str(self.api_socket),
                   JCODE_DESKTOP_CONTROL_RUNTIME_SESSION=session_id)

    def runtime_debug(self, *args):
        result = subprocess.run([*self.runtime_cli, "debug", *args, "--socket", str(self.legacy_socket)],
                                env=self.runtime_env, cwd=self.profile / "home", capture_output=True,
                                text=True, check=True, timeout=20)
        return result.stdout

    def harness_requests(self, commands):
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(10)
            client.connect(str(self.api_socket))
            with client.makefile("rb") as reader:
                def request(req, **fields):
                    nonlocal sequence
                    sequence += 1
                    client.sendall(json.dumps(dict(v=1, id=sequence, req=req, **fields)).encode() + b"\n")
                    for line in reader:
                        reply = json.loads(line)
                        if reply.get("reply_to") == sequence:
                            if reply["ev"] == "error":
                                raise RuntimeError(f"{req}: {reply}")
                            return reply
                    raise RuntimeError(f"no reply to {req}")

                sequence = 0
                request("hello", min_version=1, max_version=1, client="desktop-fixture")
                return [request(req, **fields) for req, fields in commands]

    def runtime_spawn(self, name, command):
        with (self.run / f"{name}.log").open("a") as log:
            process = subprocess.Popen(command, env=self.runtime_env, cwd=self.profile / "home", stdout=log, stderr=log)
        self.runtime_processes.append(process)
        return process

    @staticmethod
    def runtime_stop(process):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)

    def runtime_wait(self, predicate, label):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            result = predicate()
            if result:
                return result
            time.sleep(.15)
        raise RuntimeError(f"timed out waiting for {label}")

    def start_background_daemon(self):
        self.daemon = self.runtime_spawn("private-daemon", [*self.runtime_cli, "serve", "--socket", str(self.legacy_socket)])
        self.runtime_wait(self.legacy_socket.exists, "private daemon socket")
        self.api_bridge = self.runtime_spawn("private-bridge", [self.manifest["bridge_bin"], str(self.api_socket), str(self.legacy_socket)])
        self.runtime_wait(self.api_socket.exists, "private bridge socket")

    def background_jobs_journey(self):
        if self.manifest["fixture"] != "background-jobs":
            raise RuntimeError("journey requires the background-jobs fixture")
        session = self.target["session_id"]
        def jobs():
            return self.drive("state")["panel"]["background_jobs"]
        self.runtime_debug("message", "[e2e:reload-bg:desktop] verify-no-bg-wake verify-bg-success", "--session", session, "--wait")
        running = self.runtime_wait(lambda: next((job for job in jobs()["tasks"] if job["status"] == "running"), None), "running task in Desktop")
        task_id = running["task_id"]
        self.drive("click", {"selector": "swarm-status"})
        self.drive("click", {"selector": f"background-job:{task_id}"})
        if jobs()["selected"] != task_id:
            raise RuntimeError("native click did not open task output")
        self.screenshot("background-jobs-running.png")
        # After reconnect, progress and output must still reach the same open detail.
        self.runtime_stop(self.api_bridge)
        self.runtime_stop(self.daemon)
        self.api_socket.unlink(missing_ok=True)
        self.legacy_socket.unlink(missing_ok=True)
        self.start_background_daemon()
        (self.profile / "desktop.progress").touch()
        def saw_progress():
            state = jobs()
            return (state["selected"] == task_id and any(
                job["task_id"] == task_id and (job.get("progress") or {}).get("percent") == 50
                for job in state["tasks"]))
        self.runtime_wait(saw_progress, "task progress after daemon and bridge restart")
        (self.profile / "desktop.release").touch()
        def completed():
            state = jobs()
            return (state if any(job["task_id"] == task_id and job["status"] == "completed"
                                 for job in state["tasks"])
                    and "completed by original shell" in (state["output"] or {}).get("tail", "") else None)
        result = self.runtime_wait(completed, "completed status and latest log in open detail")
        frame = self.screenshot("background-jobs-completed.png")
        report = {"journey": "background-jobs", "passed": True, "start": running,
                  "after_restart": result, "screenshot": frame}
        save(self.run / "background-jobs-proof.json", report)
        return report

    def swarm_dm_journey(self):
        if self.manifest["fixture"] not in ("background-jobs", "swarm-dm"):
            raise RuntimeError("swarm-dm needs the private daemon from --fixture swarm-dm")
        recipient = self.target["session_id"]
        sender = json.loads(self.runtime_debug(f"create_session:{self.profile / 'home'}"))["session_id"]
        if sender == recipient:
            raise RuntimeError("DM sender must be a separate session")
        draft = "Keep this unsent draft"
        body = (
            "Please check the build output before sending the final report. "
            "The narrow layout must keep this entire preview on one clipped line."
            "\n\n**Hidden report:** the regression test caught the missing disclosure."
            "\n\n- Review the captured screenshot.\n- Keep the draft intact."
        )
        self.drive("set_composer", {"text": draft})
        save(self.run / "swarm-dm-before.json", self.drive("state"))

        def send_dm(message, delivery):
            command = {"type": "comm_message", "id": 1, "from_session": sender,
                       "to_session": recipient, "message": message, "delivery": delivery,
                       "require_target": True}
            with socket.socket(socket.AF_UNIX) as conn:
                conn.settimeout(10)
                conn.connect(str(self.legacy_socket))
                conn.sendall(json.dumps(command).encode() + b"\n")
                with conn.makefile("rb") as reader:
                    for line in reader:
                        reply = json.loads(line)
                        if reply.get("id") == command["id"] and reply.get("type") in ("done", "error"):
                            if reply["type"] != "done":
                                raise RuntimeError(f"swarm DM failed: {reply['message']}")
                            return
            raise RuntimeError("swarm DM ended without an acknowledgment")

        sent_at = time.time_ns() // 1_000_000
        send_dm(body, "notify")

        def received():
            state = self.drive("state")
            panel = state["panel"]
            for index, item in enumerate(panel["loaded_transcript_items"]):
                message = item.get("AgentMessage", {}) if isinstance(item, dict) else {}
                name = message.get("from_name")
                if (name and message["from_session"] == sender
                        and message["message"] == f"DM from {name}: {body}"):
                    if panel["session_id"] != recipient or panel["composer"] != draft:
                        raise RuntimeError("swarm DM reached the wrong session or replaced the draft")
                    if not sent_at <= message.get("timestamp_ms", 0) <= time.time_ns() // 1_000_000:
                        raise RuntimeError("live DM did not store its actual arrival time")
                    if not (self.run / "swarm-dm-arrival.png").exists():
                        # Force a frame after the async event in the hidden window.
                        self.screenshot("swarm-dm-arrival.png")
                        panel = self.drive("state")["panel"]
                    row = next((row for row in panel["visible_transcript_items"] or []
                                if row["item"] == item), None)
                    if row:
                        return state, name, index, row["selector"]
            return None

        after, name, index, selector = self.runtime_wait(received, "signed DM in the recipient's native transcript")
        save(self.run / "swarm-dm-after.json", after)
        key = f"details:{index}"
        if key in after["panel"]["expanded_regions"]:
            raise RuntimeError("new DM is not collapsed")
        collapsed = self.screenshot("swarm-dm-collapsed.png", selector)
        self.drive("click", {"selector": f"agent-message:summary:{index}"})
        expanded_state = self.drive("state")
        if key not in expanded_state["panel"]["expanded_regions"]:
            raise RuntimeError("click did not expand the DM")
        expanded = self.screenshot("swarm-dm-expanded.png", selector)
        self.drive("click", {"selector": f"agent-message:summary:{index}"})
        collapsed_state = self.drive("state")
        if key in collapsed_state["panel"]["expanded_regions"] or collapsed_state["panel"]["composer"] != draft:
            raise RuntimeError("second click did not collapse the DM or keep the draft")
        save(self.run / "swarm-dm-expanded.json", expanded_state)
        save(self.run / "swarm-dm-recollapsed.json", collapsed_state)
        if collapsed["sha256"] == expanded["sha256"]:
            raise RuntimeError("DM screenshots did not change after disclosure")

        hidden_text = "the regression test caught the missing disclosure"
        self.drive("reveal", {"row": {"kind": "text", "text": hidden_text}})
        highlighted_state = self.drive("state")
        panel = highlighted_state["panel"]
        if key not in panel["expanded_regions"] or panel["highlighted_transcript_items"] != [index, index]:
            raise RuntimeError("revealing hidden text did not expand and highlight the DM")
        highlighted = self.screenshot("swarm-dm-highlighted.png", selector)
        save(self.run / "swarm-dm-highlighted.json", highlighted_state)

        self.drive("click", {"selector": f"agent-message:sender:{index}"})
        sender_state = self.drive("state")
        sender_panels = [panel for row in sender_state["navigation"]["rows"] for panel in row["panels"]
                         if panel["session"] == sender and not panel["closing"]]
        if (len(sender_panels) != 1 or not sender_panels[0]["focused"]
                or sender_state["panel"]["composer"] != draft):
            raise RuntimeError("sender click did not open and focus its session or keep the draft")
        save(self.run / "swarm-dm-sender.json", sender_state)
        request_log = self.run / "fake-requests.jsonl"
        if request_log.exists() and "Please check the build output" in request_log.read_text():
            raise RuntimeError("notify delivery unexpectedly started a provider turn")

        self.drive("focus")
        self.drive("click", {"selector": f"agent-message:summary:{index}"})
        outgoing_text = "native_dm_verification"
        self.drive("set_composer", {"text": f"[e2e:worker-dm:{sender}:{outgoing_text}]"})
        self.drive("send")
        self.drive("set_composer", {"text": draft})

        def outgoing(panel):
            return [item["Tool"] for item in panel["loaded_transcript_items"]
                    if isinstance(item, dict) and "Tool" in item
                    and item["Tool"]["name"] == "swarm"
                    and outgoing_text in item["Tool"]["input"]]

        def outgoing_ready():
            state = self.drive("state")
            panel = state["panel"]
            calls = outgoing(panel)
            if len(calls) > 1:
                raise RuntimeError("a single tool execution produced duplicate outgoing rows")
            if len(calls) == 1 and calls[0]["done"]:
                if calls[0]["error"]:
                    raise RuntimeError(f"outgoing tool reported {calls[0]['error']}")
                if panel["composer"] != draft or any(
                    item.get("AgentMessage", {}).get("message", "").endswith(outgoing_text)
                    for item in panel["loaded_transcript_items"] if isinstance(item, dict)
                ):
                    raise RuntimeError("sending created an incoming echo or changed the draft")
                return state, calls[0]["call_id"]
            return None

        live, call_id = self.runtime_wait(outgoing_ready, "one live outgoing swarm tool row")
        self.screenshot("swarm-dm-live-pair.png")
        save(self.run / "swarm-dm-live-pair.json", live)
        self.screenshot("swarm-dm-live-outgoing.png", f"tool:{call_id}")

        # New notifications are not legacy user rows. Seed the legacy
        # envelope through the real session history path, not the UI fixture.
        replay_body = "Persisted incoming DM\n\n**Markdown survives a fresh attach.**"
        replay_sent_at = time.time_ns() // 1_000_000
        self.runtime_debug("message", f"DM from {name}: {replay_body}",
                           "--session", recipient, "--wait")
        replay_before_restart = time.time_ns() // 1_000_000
        live_pid = self.process.pid
        self.runtime_stop(self.process)
        env = json.loads((self.run / "isolation.json").read_text())["environment"]
        self.start_desktop(env, [], suffix="-replay")

        def replay_ready():
            state = self.drive("state")
            panel = state["panel"]
            calls = outgoing(panel)
            incoming = [(index, item) for index, item in enumerate(panel["loaded_transcript_items"])
                        if isinstance(item, dict) and item.get("AgentMessage", {}).get("message")
                        == f"DM from {name}: {replay_body}"]
            if len(calls) != 1 or len(incoming) != 1:
                return None
            index, item = incoming[0]
            at = item["AgentMessage"].get("timestamp_ms", 0)
            if not replay_sent_at <= at <= replay_before_restart:
                raise RuntimeError("replay did not use the persisted DM timestamp")
            return index, item

        replay_index, replay_item = self.runtime_wait(replay_ready, "fresh attach with both DM rows")
        self.screenshot("swarm-dm-replay-pair.png")
        replay = self.drive("state")
        replay_selector = next(row["selector"] for row in replay["panel"]["visible_transcript_items"]
                               if row["item"] == replay_item)
        save(self.run / "swarm-dm-replay.json", replay)
        self.screenshot("swarm-dm-replay-incoming.png", replay_selector)
        self.screenshot("swarm-dm-replay-outgoing.png", f"tool:{call_id}")
        self.drive("reveal", {"row": {"kind": "text", "text": "Markdown survives a fresh attach."}})
        if f"details:{replay_index}" not in self.drive("state")["panel"]["expanded_regions"]:
            raise RuntimeError("search did not expand the replayed DM")

        report = {"journey": "swarm-dm", "passed": True, "from_session": sender,
                  "from_name": name, "recipient": recipient, "draft_preserved": True,
                  "sender_opened": True, "collapsed": collapsed, "expanded": expanded,
                  "highlighted": highlighted, "outgoing_call_id": call_id,
                  "live_pid": live_pid, "replay_pid": self.process.pid,
                  "legacy_replay_timestamp_ms": replay_item["AgentMessage"]["timestamp_ms"]}
        save(self.run / "swarm-dm-proof.json", report)
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
        if name == "swarm-status":
            if not before["panel"]["workers_open"]:
                raise RuntimeError("status pill did not open the worker list")
            labels = {row["session_id"]: row["label"] for row in before["panel"]["workers"]}
            if labels.get(SWARM_WORKER) != "btw-enter":
                raise RuntimeError("auto-lane worker must show its catalog title without an agent label")
            if labels.get("swarm-fixture-2") != "Test runner":
                raise RuntimeError("finished row used the retry prompt instead of the lane label")
            click("finished-workers")
            expanded = self.screenshot("swarm-status-expanded.png")
            click("finished-workers")
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
        if name == "swarm-status":
            if expanded["sha256"] == first["sha256"]:
                raise RuntimeError("opening the finished section did not change the native frame")
            report["expanded"] = expanded
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
        if self.runtime_processes:
            (self.profile / "desktop.progress").touch()
            (self.profile / "desktop.release").touch()
            for process in reversed(self.runtime_processes):
                self.runtime_stop(process)
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
    if args.fullscreen_layout and args.fixture != "tabbar":
        raise RuntimeError("--fullscreen-layout requires --fixture tabbar")
    if args.no_sidebar and args.fixture != "tabbar":
        raise RuntimeError("--no-sidebar requires --fixture tabbar")
    if args.fixture in ("background-jobs", "swarm-dm", "side-question-history") and not all(
        path and path.is_file() and os.access(path, os.X_OK) for path in (args.jcode_bin, args.bridge_bin)
    ):
        raise RuntimeError(f"{args.fixture} requires --jcode-bin and --bridge-bin from the sibling checkout")
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
        # Keep the process and doctor tied to the same bytes if a later build
        # replaces the mutable target executable while this run is alive.
        binary = Path(shutil.copy2(binary, scratch / "jcode-desktop"))
        version = subprocess.check_output([str(binary), "--version"], env=env, text=True, timeout=10).strip()
    except BaseException:
        shutil.rmtree(scratch)
        raise
    manifest = {"schema": SCHEMA, "phase": "starting", "evidence": str(run), "scratch": str(scratch),
                "supervisor_socket": str(scratch / "supervisor.sock"), "token": secrets.token_hex(32),
                "binary": str(binary), "binary_sha256": digest(binary), "version": version,
                "sources": sources, "build": command, "fixture": args.fixture, "theme": args.theme,
                "fullscreen_layout": args.fullscreen_layout,
                "no_sidebar": args.no_sidebar,
                "persist_restart_manifest": args.persist_restart_manifest,
                "window_width": args.window_width,
                "queued": max(args.queued, 2 if args.queue_edit else 0),
                "queue_edit": args.queue_edit,
                "lifetime_seconds": args.lifetime,
                "jcode_bin": str(args.jcode_bin.resolve()) if args.jcode_bin else None,
                "bridge_bin": str(args.bridge_bin.resolve()) if args.bridge_bin else None}
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
            child.add_argument("--theme", choices=("neutral-dark", "neutral-light"),
                               help="override the theme in this private profile")
            child.add_argument("--fixture", choices=("all", "empty", "expandable", "reasoning", *ASK_FIXTURES, "swarm-worktree", "image-prompt", "gallery-preload", "gallery-navigation", "compaction", "orb-todos", "tabbar", "tab-statuses", "sidebar-todos", "mode-picker", "transcript-scroll", "draft-session", "background-tasks", "background-jobs", "swarm-dm", "side-question-history", "cache-card", "resume-preview", "voice-rt", "openrig", "openrig-live", "openrig-live-split", "files-panel"), default="expandable")
            child.add_argument("--fullscreen-layout", action="store_true",
                               help="render the tabbar fixture without traffic-light insets, without entering a macOS Space")
            child.add_argument("--no-sidebar", action="store_true",
                               help="render the tabbar fixture with the sidebar closed")
            child.add_argument("--persist-restart-manifest", action="store_true",
                               help="exercise real restart manifests in the private profile")
            child.add_argument("--window-width", type=int, choices=range(360, 1601), metavar="360..1600",
                               help="set the hidden native window's initial width in points")
            child.add_argument("--jcode-bin", type=Path)
            child.add_argument("--bridge-bin", type=Path)
            child.add_argument("--queued", type=int, choices=range(0, 9), default=0, metavar="0..8",
                               help="seed N queued prompts above the composer")
            child.add_argument("--queue-edit", action="store_true",
                               help="open the first queued prompt in its inline editor, with a screenshot and files (implies --queued 2)")
            child.add_argument("--lifetime", type=int, choices=range(30, 3601), default=900, metavar="30..3600")
        elif command == "drive":
            group = child.add_mutually_exclusive_group(required=True)
            group.add_argument("--journey", choices=("tools", "background-tasks", "background-jobs", "composer", "composer-arrows", "composer-advanced", "composer-footer", "search", "compaction", "tab-statuses", "mode-picker", "transcript-scroll", "draft-session", "side-question", "side-question-history", "resume-preview", "swarm-dm", *SWARM_JOURNEYS))
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
