#!/usr/bin/env python3
"""Safety/contract tests. These do not claim native UI acceptance."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
SPEC = importlib.util.spec_from_file_location("verify_desktop", Path(__file__).with_name("verify-jcode-desktop.py"))
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


class IsolationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.run = self.root / "evidence"
        self.run.mkdir(mode=0o700)
        self.scratch = self.root / "private"
        self.scratch.mkdir(mode=0o700)
        self.manifest = {"schema": 1, "evidence": str(self.run), "scratch": str(self.scratch),
                         "supervisor_socket": str(self.scratch / "supervisor.sock"),
                         "phase": "starting", "token": "test-run-token"}
        (self.scratch / ".owner").write_text(self.manifest["token"])
        (self.scratch / ".owner").chmod(0o600)
        self.instance = VERIFY.Instance(self.run, self.manifest)
        self.instance.target = {"window_id": "123:0", "session_id": "private-chat"}

    def test_environment_does_not_inherit_user_sockets_or_credentials(self):
        with patch.dict(os.environ, {"JCODE_SOCKET": "/real/daemon.sock", "JCODE_DESKTOP_CONTROL_DIR": "/real/desktop",
                                     "OPENAI_API_KEY": "not-a-real-key", "DISPLAY": ":0", "SSH_AUTH_SOCK": "/real/agent"}):
            env = VERIFY.environment(self.scratch / "profile", self.scratch / "control")
        self.assertEqual(env["JCODE_SOCKET"], str(self.scratch / "profile/runtime/daemon.sock"))
        self.assertEqual(env["JCODE_DESKTOP_CONTROL_DIR"], str(self.scratch / "control"))
        self.assertEqual(env["JCODE_DESKTOP_SCREENSHOT"], "1")
        self.assertEqual(env["JCODE_DESKTOP_CONTROL_TEST"], "1")
        for key in ("OPENAI_API_KEY", "DISPLAY", "WAYLAND_DISPLAY", "SSH_AUTH_SOCK", "DYLD_LIBRARY_PATH"):
            self.assertNotIn(key, env)

    def test_reuses_acceptance_transport_and_environment(self):
        self.assertEqual(Path(VERIFY.NATIVE.__file__).name, "accept-native-control.py")
        self.assertEqual(VERIFY.isolated_env.__module__, "screenshot")

    def test_unsupported_platform_refuses_before_launch(self):
        with patch.object(VERIFY.sys, "platform", "linux"):
            with self.assertRaisesRegex(RuntimeError, "refusing launch"):
                VERIFY.require_safe_host()

    def test_missing_hidden_gate_refuses_before_launch(self):
        with patch.object(VERIFY.sys, "platform", "darwin"), patch.object(Path, "read_text", return_value="no hidden gate"):
            with self.assertRaisesRegex(RuntimeError, "hidden/non-activating"):
                VERIFY.require_safe_host()

    def test_routing_and_permission_override_refused_before_transport(self):
        with patch.object(self.instance, "native") as native:
            for field in ("target", "destination", "path", "v", "action", "allow_user_window"):
                with self.assertRaises(ValueError):
                    self.instance.drive("click", {field: "foreign"})
            native.assert_not_called()

    def test_open_session_forces_background_in_owned_window(self):
        with patch.object(self.instance, "native", return_value={}) as native:
            self.instance.drive("open_session", {"session_id": "fixture-session"})
        self.assertEqual(native.call_args.args[0], {"action": "open_session", "session_id": "fixture-session",
                         "destination": {"background_panel": {"window_id": "123:0"}}})

    def test_set_composer_targets_only_owned_window(self):
        with patch.object(self.instance, "native", return_value={}) as native:
            self.instance.drive("set_composer", {"text": "draft"})
        self.assertEqual(native.call_args.args[0]["target"], self.instance.target)
        self.assertTrue(native.call_args.args[0]["allow_user_window"])

    def test_unknown_action_and_long_wait_refused(self):
        with patch.object(self.instance, "native") as native:
            for action, params in (("reload", {}), ("wait_for", {"timeout_ms": 120000})):
                with self.assertRaises(ValueError):
                    self.instance.drive(action, params)
            native.assert_not_called()

    def test_screenshot_cannot_write_outside_evidence(self):
        with patch.object(self.instance, "native") as native:
            for name in ("../screen.png", "/tmp/screen.png", "screen.jpg", ""):
                with self.assertRaises(ValueError):
                    self.instance.screenshot(name)
            native.assert_not_called()

    def test_journey_cannot_replace_existing_evidence(self):
        proof = self.run / "tools-before.json"
        proof.write_text("original proof")
        with patch.object(self.instance, "doctor") as doctor:
            with self.assertRaisesRegex(RuntimeError, "evidence already exists"):
                self.instance.journey("tools")
            doctor.assert_not_called()
        self.assertEqual(proof.read_text(), "original proof")

    def test_foreign_socket_is_rejected_without_sending(self):
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        self.addCleanup(lambda: self.stop(child))
        self.instance.process = child
        self.instance.endpoint = self.root / "foreign.sock"
        with patch.object(VERIFY.NATIVE, "request") as request:
            with self.assertRaisesRegex(RuntimeError, "refusing endpoint"):
                self.instance.native({"action": "list"})
            request.assert_not_called()

    def test_cleanup_terminates_only_popen_child_not_manifest_pid_and_keeps_proof(self):
        own = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        foreign = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        self.addCleanup(lambda: self.stop(own))
        self.addCleanup(lambda: self.stop(foreign))
        self.instance.process = own
        self.manifest["desktop_pid"] = foreign.pid
        evidence = self.run / "proof.png"
        evidence.write_bytes(b"retained screenshot")
        result = self.instance.cleanup("test")
        self.assertIsNotNone(own.poll())
        self.assertIsNone(foreign.poll())
        self.assertFalse(self.scratch.exists())
        self.assertEqual(evidence.read_bytes(), b"retained screenshot")
        self.assertTrue(result["cleaned"])
        self.assertEqual(VERIFY.rpc(self.run, "cleanup")["desktop_pid"], own.pid)
        with self.assertRaisesRegex(RuntimeError, "instance is cleaned"):
            VERIFY.rpc(self.run, "doctor")

    def test_manifest_permissions_and_symlinks_refused(self):
        VERIFY.save(self.run / "run.json", self.manifest)
        self.assertEqual(VERIFY.read_manifest(self.run)["schema"], 1)
        (self.run / "run.json").chmod(0o644)
        with self.assertRaisesRegex(RuntimeError, "unsafe run manifest"):
            VERIFY.read_manifest(self.run)
        (self.run / "run.json").unlink()
        target = self.root / "elsewhere.json"
        target.write_text(json.dumps(self.manifest))
        target.chmod(0o600)
        (self.run / "run.json").symlink_to(target)
        with self.assertRaisesRegex(RuntimeError, "unsafe run manifest"):
            VERIFY.read_manifest(self.run)

    def test_binary_digest_works_on_system_python(self):
        path = self.run / "test.bin"
        path.write_bytes(b"abc")
        self.assertEqual(VERIFY.digest(path), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")

    def test_cleanup_refuses_foreign_scratch_before_any_termination(self):
        (self.scratch / ".owner").write_text("foreign run")
        with patch.object(VERIFY.subprocess, "Popen") as child:
            self.instance.process = child
            with self.assertRaisesRegex(RuntimeError, "not owned by this run"):
                self.instance.cleanup("test")
            child.terminate.assert_not_called()
        self.assertTrue(self.scratch.exists())

    def test_duplicate_supervisor_does_not_cleanup_existing_run(self):
        self.manifest["phase"] = "ready"
        VERIFY.save(self.run / "run.json", self.manifest)
        with patch.object(VERIFY.Instance, "cleanup") as cleanup:
            with self.assertRaisesRegex(RuntimeError, "duplicate supervisor"):
                VERIFY.supervise(self.run)
            cleanup.assert_not_called()
        self.assertTrue(self.scratch.exists())

    @staticmethod
    def stop(process):
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
