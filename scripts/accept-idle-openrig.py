#!/usr/bin/env python3
"""Measure isolated idle Desktop process CPU and confirm zero unsolicited OpenRig reads."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import threading
import time


def cpu_seconds(pid):
    field = subprocess.check_output(["/bin/ps", "-p", str(pid), "-o", "time="], text=True)
    value = 0.0
    for part in field.strip().split(":"):
        value = value * 60 + float(part)
    return value


def observe(binary):
    requests = []

    class Fixture(BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            body = json.dumps({"selfHostId": "isolated-host", "hosts": [], "rigs": [], "nodes": [],
                               "status": str(len(requests)), "items": []}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    with ThreadingHTTPServer(("127.0.0.1", 0), Fixture) as server, tempfile.TemporaryDirectory(
        prefix="jdc-idle-", dir="/tmp"
    ) as path:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
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
            "JCODE_DESKTOP_TEST_OPENRIG_POLL": "1",
            "OPENRIG_URL": f"http://127.0.0.1:{server.server_port}",
        }
        with subprocess.Popen(
            [os.path.abspath(binary), "--new-process", "--no-hot-reload"],
            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        ) as proc:
            try:
                startup_deadline = time.monotonic() + 15
                while not list((root / "control").glob("*.sock")):
                    if proc.poll() is not None or time.monotonic() >= startup_deadline:
                        raise AssertionError("hidden Desktop never started")
                    time.sleep(0.05)
                time.sleep(4)
                start_count = len(requests)
                start_cpu = cpu_seconds(proc.pid)
                start = time.monotonic()
                time.sleep(12)
                cpu = 100 * (cpu_seconds(proc.pid) - start_cpu) / (time.monotonic() - start)
                return len(requests) - start_count, cpu
            finally:
                if proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                server.shutdown()
                thread.join()


if __name__ == "__main__":
    if sys.platform != "darwin":
        sys.exit("macOS is required")
    candidate, *baseline = sys.argv[1:]
    before = []
    after = []
    for _ in range(3 if baseline else 1):
        if baseline:
            before.append(observe(baseline[0]))
        after.append(observe(candidate))
    for label, runs in (("baseline", before), ("candidate", after)):
        if runs:
            print(f"{label}: {runs}, median core CPU {statistics.median(cpu for _, cpu in runs):.3f}%")
    assert all(n > 0 for n, _ in before), "baseline did not exercise the OpenRig transport"
    assert all(n == 0 for n, _ in after), "ordinary idle Desktop polled OpenRig"
