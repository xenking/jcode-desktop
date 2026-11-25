#!/usr/bin/env python3
"""Native macOS check: a hidden Desktop writes its actual blocked main stack."""

import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


def wait_for(proc, read, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise AssertionError(f"Desktop exited ({proc.returncode})")
        if result := read():
            return result
        time.sleep(0.05)
    raise AssertionError("timed out waiting for Desktop")


def check(binary, *, block):
    with tempfile.TemporaryDirectory(prefix="jdc-hang-", dir="/tmp") as path:
        root = Path(path)
        for name in ("home", "runtime", "tmp", "jcode", "control"):
            (root / name).mkdir(mode=0o700)
        env = {
            "PATH": "/usr/bin:/bin",
            "HOME": str(root / "home"),
            "XDG_RUNTIME_DIR": str(root / "runtime"),
            "TMPDIR": str(root / "tmp"),
            "JCODE_HOME": str(root / "jcode"),
            "JCODE_TEST_SESSION": "1",
            "JCODE_DESKTOP_SCREENSHOT": "1",
            "JCODE_DESKTOP_CONTROL_TEST": "1",
            "JCODE_DESKTOP_CONTROL_DIR": str(root / "control"),
            "JCODE_DESKTOP_STATE": str(root / "state"),
        }
        if block:
            env["JCODE_DESKTOP_TEST_MAIN_HANG"] = "1"
        reports = root / "jcode/logs/desktop-hangs"
        with subprocess.Popen(
            [binary, "--single-panel", "--new-process", "--no-hot-reload"],
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        ) as proc:
            try:
                wait_for(proc, lambda: list((root / "control").glob("*.sock")), 15)
                if not block:
                    time.sleep(4)
                    assert proc.poll() is None
                    assert not list(reports.glob("*.txt")), "healthy main produced a report"
                    return
                report, = wait_for(proc, lambda: list(reports.glob("*.txt")), 18)
                assert report.stat().st_size <= 2 * 1024 * 1024, "unbounded report"
                assert report.stat().st_mode & 0o777 == 0o600, "report is not private"
                text = report.read_text(errors="replace")
                main = text.split("com.apple.main-thread", 1)[1]
                # Frame records start with '+'; the next real thread is a plain count.
                main = re.split(r"\n    \d+ Thread_", main, maxsplit=1)[0]
                assert "block_main_for_test" in main, main[:2000]
                assert list(reports.glob("*.txt")) == [report], "duplicate report"
                print(f"main stall captured from pid {proc.pid}: {report.stat().st_size} bytes")
            finally:
                if proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        proc.kill()


if __name__ == "__main__":
    if sys.platform != "darwin":
        sys.exit("macOS is required")
    binary = os.path.abspath(sys.argv[1])
    check(binary, block=False)
    check(binary, block=True)
