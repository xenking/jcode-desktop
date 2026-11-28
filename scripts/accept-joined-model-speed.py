#!/usr/bin/env python3
"""Drive the joined model and speed controls in isolated native Desktop windows."""
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


def verify(binary, artifacts, theme, width):
    with tempfile.TemporaryDirectory(prefix="jdc-speed-", dir="/tmp") as directory:
        root = Path(directory)
        for name in ("home", "runtime", "config", "cache", "data", "state", "jcode", "tmp"):
            (root / name).mkdir(mode=0o700)
        config = root / "desktop.toml"
        config.write_text(f'[appearance]\ntheme = "{theme}"\nreduce_motion = true\n')
        env = {
            "PATH": "/usr/bin:/bin", "HOME": str(root / "home"), "TMPDIR": str(root / "tmp"),
            "XDG_RUNTIME_DIR": str(root / "runtime"), "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_CACHE_HOME": str(root / "cache"), "XDG_DATA_HOME": str(root / "data"),
            "XDG_STATE_HOME": str(root / "state"), "JCODE_HOME": str(root / "jcode"),
            "JCODE_TEST_SESSION": "1", "JCODE_NO_TELEMETRY": "1",
            "JCODE_DESKTOP_CONFIG": str(config), "JCODE_DESKTOP_STATE": str(root / "desktop-state"),
            "JCODE_DESKTOP_SCREENSHOT": "1", "JCODE_DESKTOP_SCREENSHOT_COMPOSER_PICKER": "model",
            "JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT": "voice-rt", "JCODE_DESKTOP_SCREENSHOT_BAR": "high",
            "JCODE_DESKTOP_CONTROL_TEST": "1", "JCODE_DESKTOP_CONTROL_DIR": str(root / "control"),
            "JCODE_DESKTOP_CONTROL_WINDOW_WIDTH": str(width), "LANG": "C.UTF-8",
        }
        logfile = artifacts / f"{theme}-{width}.log"
        with logfile.open("w") as log:
            process = subprocess.Popen(
                [str(binary), "--single-panel", "--new-process", "--no-hot-reload"],
                env=env, stdout=log, stderr=log,
            )
            try:
                deadline = time.monotonic() + 20
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

                def capture(name):
                    png = artifacts / f"{theme}-{width}-{name}.png"
                    native("render", selector=None, path=str(png))
                    assert png.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"), png

                def click(selector):
                    native("click", selector=selector, allow_user_window=True)

                def key(name):
                    native("press_key", key=f"fixture:{name}", allow_user_window=True)

                def state():
                    return native("state")["panel"]

                def expect(**values):
                    for _ in range(40):
                        panel = state()
                        observed = {key: panel.get(key) for key in values}
                        if observed == values:
                            return panel
                        time.sleep(.05)
                    raise AssertionError((values, observed))

                def absent(selector):
                    result = request(endpoint, action="click", target=target, selector=selector,
                                     allow_user_window=True)
                    assert result.get("code") == "selector_not_visible", result

                start_model = "gpt-6.2-astra"
                alternate = "gpt-6.2-sol"
                expect(picker="model", model=start_model, reasoning_effort="high")
                capture("initial")
                click(f"model-speed:{start_model}:Fast")
                expect(picker="model", model=start_model, speed_mode="fast")
                capture("fast-dictating")
                click(f"model-speed:{start_model}:Ultrafast")
                expect(picker="model", model=start_model, speed_mode="ultrafast")
                capture("ultrafast-dictating")
                click(f"model-speed:{start_model}:Ultrafast")
                expect(picker="model", model=start_model, speed_mode="standard")
                absent("panel-model-speed")
                absent(f"model-speed:{alternate}:Fast")
                click(f"model-speed:{alternate}:Ultrafast")
                expect(picker=None, model=alternate, speed_mode="ultrafast")
                capture("alternate-dictating")
                click("panel-model-speed")
                expect(picker=None, model=alternate, speed_mode="standard")
                click("composer.voice")
                expect(composer="Before spoken words after", composer_focus=True)

                for command, expected in (("/speed standard", "standard"), ("/speed ultrafast", "ultrafast"),
                                          ("/speed standard", "standard")):
                    native("set_composer", text=command, allow_user_window=True)
                    key("enter")
                    expect(speed_mode=expected, composer="", composer_focus=True)
                absent("panel-model-speed")
                click("panel-model-effort")
                expect(picker="effort")
                capture("effort-standard")
                key("escape")
                expect(picker=None)

                native("set_composer", text="Unsent keyboard draft", allow_user_window=True)
                click("panel-model")
                expect(picker="model")
                key("up")
                key("alt-shift-right")
                expect(picker="model", model=alternate, speed_mode="ultrafast")
                capture("keyboard")
                key("alt-shift-left")
                expect(picker="model", model=alternate, speed_mode="standard")
                key("escape")
                expect(picker=None, composer="Unsent keyboard draft")
            finally:
                process.terminate()
                process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("artifacts", type=Path)
    args = parser.parse_args()
    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    errors = []
    for theme in ("neutral-dark", "neutral-light"):
        for width in (360, 900):
            try:
                verify(args.binary.resolve(), artifacts, theme, width)
                print(f"PASS {theme}/{width}")
            except AssertionError as error:
                errors.append(f"{theme}/{width}: {error}")
    if errors:
        raise SystemExit("\n".join(errors))


if __name__ == "__main__":
    main()
