#!/usr/bin/env python3
"""Verify the mixed Clean Chat replay in hidden native Desktop windows."""
import argparse
import json
from pathlib import Path
import re
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
            reply = json.loads(stream.readline())
    assert reply["status"] == "ok", reply
    return reply["result"]


def capture(binary, theme, clean, output):
    with tempfile.TemporaryDirectory(prefix="jdc-chat-", dir="/tmp") as directory:
        root = Path(directory)
        for name in ("home", "runtime", "config", "cache", "data", "state", "jcode", "tmp"):
            (root / name).mkdir(mode=0o700)
        config = root / "desktop.toml"
        config.write_text(
            f'[appearance]\ntheme = "{theme}"\nreduce_motion = true\n'
            f'[display]\nclean_chat_default = {str(clean).lower()}\n'
        )
        env = {
            "PATH": "/usr/bin:/bin", "HOME": str(root / "home"), "TMPDIR": str(root / "tmp"),
            "XDG_RUNTIME_DIR": str(root / "runtime"), "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_CACHE_HOME": str(root / "cache"), "XDG_DATA_HOME": str(root / "data"),
            "XDG_STATE_HOME": str(root / "state"), "JCODE_HOME": str(root / "jcode"),
            "JCODE_DESKTOP_CONFIG": str(config), "JCODE_DESKTOP_STATE": str(root / "desktop-state"),
            "JCODE_DESKTOP_SCREENSHOT": "1", "JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT": "clean-chat",
            "JCODE_DESKTOP_CONTROL_TEST": "1", "JCODE_DESKTOP_CONTROL_DIR": str(root / "control"),
            "JCODE_TEST_SESSION": "1", "JCODE_NO_TELEMETRY": "1", "LANG": "C.UTF-8",
        }
        with (root / "log").open("w+") as log:
            proc = subprocess.Popen(
                [str(binary), "--single-panel", "--new-process", "--no-hot-reload"],
                env=env, stdout=log, stderr=log,
            )
            try:
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline and proc.poll() is None:
                    sockets = list((root / "control").glob("*.sock"))
                    if sockets:
                        break
                    time.sleep(0.05)
                else:
                    log.seek(0)
                    raise RuntimeError(f"Private native Desktop did not start:\n{log.read()}")
                endpoint = sockets[0]
                listing = request(endpoint, action="list")
                target = {"window_id": listing["window_id"], "session_id": "screenshot-fixture"}
                request(endpoint, action="render", target=target, selector=None, path=str(output))
            finally:
                proc.terminate()
                proc.wait(timeout=10)
    return subprocess.run(
        ["tesseract", str(output), "stdout", "--psm", "11"], capture_output=True, text=True, check=True,
    ).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("artifacts", type=Path)
    args = parser.parse_args()
    destination = args.artifacts.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    for theme in ("neutral-dark", "neutral-light"):
        full = capture(args.binary.resolve(), theme, False, destination / f"{theme}-full.png")
        clean = capture(args.binary.resolve(), theme, True, destination / f"{theme}-clean.png")
        assert "Respecting the todo lane" in full, (theme, full)
        assert "Respecting the todo lane" not in clean, (theme, clean)
        assert "Archived note" in clean and "The import is running" in clean, (theme, clean)
        assert "The import passed" in clean and "No records were dropped" in clean, (theme, clean)
        labels = re.findall(r"^(You|Update|Final)\s*$", clean, re.MULTILINE)
        assert labels == ["You", "You", "Update", "Final"], (theme, labels, clean)
        print(f"{theme}: full reasoning visible; Clean Chat shows {', '.join(labels)} and all public text")


if __name__ == "__main__":
    main()
