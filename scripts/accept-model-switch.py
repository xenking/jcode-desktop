#!/usr/bin/env python3
"""Show a queued model switch beside the chip and a cache card that names the serving model.

Drives isolated native Desktop windows from offline fixtures: no daemon, no network.
"""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time


def request(endpoint, **payload):
    with socket.socket(socket.AF_UNIX) as conn:
        conn.settimeout(10)
        conn.connect(str(endpoint))
        conn.sendall(json.dumps({"v": 1, **payload}).encode() + b"\n")
        with conn.makefile("rb") as stream:
            return json.loads(stream.readline())


def verify(binary, artifacts, stage, baseline=False):
    with tempfile.TemporaryDirectory(prefix="jdc-switch-", dir="/tmp") as directory:
        root = Path(directory)
        for name in ("home", "runtime", "config", "cache", "data", "state", "jcode", "tmp"):
            (root / name).mkdir(mode=0o700)
        config = root / "desktop.toml"
        config.write_text('[appearance]\ntheme = "neutral-dark"\nreduce_motion = true\n')
        env = {
            "PATH": "/usr/bin:/bin", "HOME": str(root / "home"), "TMPDIR": str(root / "tmp"),
            "XDG_RUNTIME_DIR": str(root / "runtime"), "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_CACHE_HOME": str(root / "cache"), "XDG_DATA_HOME": str(root / "data"),
            "XDG_STATE_HOME": str(root / "state"), "JCODE_HOME": str(root / "jcode"),
            "JCODE_TEST_SESSION": "1", "JCODE_NO_TELEMETRY": "1",
            "JCODE_DESKTOP_CONFIG": str(config), "JCODE_DESKTOP_STATE": str(root / "desktop-state"),
            "JCODE_DESKTOP_SCREENSHOT": "1", "JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT": "expandable",
            "JCODE_DESKTOP_SCREENSHOT_BAR": "high", "JCODE_DESKTOP_SCREENSHOT_CACHE_CARD": "1",
            "JCODE_DESKTOP_SCREENSHOT_MODEL_SWITCH": stage,
            "JCODE_DESKTOP_CONTROL_TEST": "1", "JCODE_DESKTOP_CONTROL_DIR": str(root / "control"),
            "JCODE_DESKTOP_CONTROL_WINDOW_WIDTH": "900", "LANG": "C.UTF-8",
        }
        logfile = artifacts / f"{stage}.log"
        with logfile.open("w") as log:
            process = subprocess.Popen(
                [str(binary), "--single-panel", "--new-process", "--no-hot-reload"],
                env=env, stdout=log, stderr=log,
            )
            try:
                deadline = time.monotonic() + 30
                while process.poll() is None and time.monotonic() < deadline:
                    sockets = list((root / "control").glob("*.sock"))
                    if sockets:
                        break
                    time.sleep(.05)
                else:
                    raise RuntimeError(f"Private Desktop did not start. See {logfile}")
                endpoint = sockets[0]
                listing = request(endpoint, action="list")
                assert listing["status"] == "ok", listing
                target = {"window_id": listing["result"]["window_id"], "session_id": "screenshot-fixture"}

                def native(action, **args):
                    result = request(endpoint, action=action, target=target, **args)
                    assert result["status"] == "ok", result
                    return result["result"]

                def capture(name, selector=None):
                    png = artifacts / f"{stage}-{name}.png"
                    native("render", selector=selector, path=str(png))
                    assert png.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"), png

                def state():
                    for _ in range(40):
                        panel = native("state")["panel"]
                        if panel.get("model"):
                            return panel
                        time.sleep(.05)
                    return panel

                panel = state()
                if stage == "queued":
                    assert panel["model"] == "claude-opus-5-5", panel
                    assert panel["pending_model"] == "gpt-6.2-sol", panel
                    capture("chip")
                    if not baseline:
                        native("click", selector="panel-model-pending", allow_user_window=True)
                else:
                    assert panel["model"] == "gpt-6.2-sol", panel
                    assert panel["pending_model"] is None, panel
                    native("click", selector="hover:panel-cache", allow_user_window=True)
                    # The tooltip opens after its hover delay.
                    time.sleep(1.2)
                    capture("cache-card")
            finally:
                process.terminate()
                process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("artifacts", type=Path)
    parser.add_argument("--baseline", action="store_true",
                        help="capture a build without the pending chip, for a before image")
    args = parser.parse_args()
    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    for stage in ("queued", "switched"):
        verify(args.binary.resolve(), artifacts, stage, args.baseline)
        print(f"PASS {stage}")


if __name__ == "__main__":
    main()
