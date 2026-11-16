#!/usr/bin/env python3
"""Exercise native control through a private, hidden offline Desktop process."""
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time


def request(path, action):
    with socket.socket(socket.AF_UNIX) as conn:
        conn.settimeout(8)
        conn.connect(str(path))
        conn.sendall(json.dumps({"v": 1, **action}).encode() + b"\n")
        with conn.makefile("rb") as stream:
            raw = stream.readline()
            try:
                return json.loads(raw)
            except json.JSONDecodeError:
                print(f"invalid socket response ({len(raw)} bytes): {raw[-512:]!r}", file=sys.stderr)
                raise


def main():
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="jdc-e2e-", dir="/tmp") as root:
        root = Path(root)
        for name in ("home", "runtime", "config", "cache", "data", "state", "jcode", "tmp"):
            (root / name).mkdir(mode=0o700)
        env = {
            "PATH": "/usr/bin:/bin", "HOME": str(root / "home"),
            "XDG_RUNTIME_DIR": str(root / "runtime"), "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_CACHE_HOME": str(root / "cache"), "XDG_DATA_HOME": str(root / "data"),
            "XDG_STATE_HOME": str(root / "state"), "JCODE_HOME": str(root / "jcode"),
            "TMPDIR": str(root / "tmp"), "JCODE_DESKTOP_SCREENSHOT": "1",
            "JCODE_DESKTOP_CONTROL_TEST": "1", "JCODE_DESKTOP_CONTROL_DIR": str(root / "control"),
            "JCODE_DESKTOP_STATE": str(root / "desktop-state"), "LANG": "C.UTF-8",
        }
        with open(root / "stderr", "w+") as log:
            proc = subprocess.Popen([str(binary), "--single-panel", "--new-process", "--no-hot-reload"], env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 30
                endpoint = None
                while time.monotonic() < deadline:
                    if proc.poll() is not None:
                        raise RuntimeError(f"Desktop exited: {proc.returncode}")
                    sockets = list((root / "control").glob("*.sock"))
                    if sockets:
                        endpoint = sockets[0]
                        break
                    time.sleep(0.05)
                if endpoint is None:
                    raise RuntimeError("private Desktop control socket did not appear")
                listing = request(endpoint, {"action": "list"})
                assert listing["status"] == "ok", listing
                window = listing["result"]["window_id"]
                panels = [panel for row in listing["result"]["navigation"]["rows"] for panel in row["panels"]]
                assert panels, listing
                target = {"window_id": window, "session_id": panels[0]["session"]}
                state = request(endpoint, {"action": "state", "target": target})
                assert state["status"] == "ok", state
                assert state["result"]["panel"]["session_id"] == target["session_id"]
                result = request(endpoint, {"action": "set_composer", "target": target, "text": "isolated draft", "allow_user_window": True})
                assert result["status"] == "ok", result
                state = request(endpoint, {"action": "state", "target": target})
                assert state["result"]["panel"]["composer"] == "isolated draft", state
                wrong = request(endpoint, {"action": "state", "target": {**target, "session_id": "wrong"}})
                assert wrong["status"] == "error", wrong
                png = root / "window.png"
                rendered = request(endpoint, {"action": "render", "target": target, "selector": None, "path": str(png)})
                if rendered["status"] == "ok":
                    assert png.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
                else:
                    assert rendered["code"] == "render_failed" and not png.exists(), rendered
                    print("macOS offscreen render unavailable")
                print("private Desktop socket list/state/set_composer passed")
            except BaseException:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()


if __name__ == "__main__":
    main()
