#!/usr/bin/env python3
"""Exercise native control through a private, hidden offline Desktop process."""
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
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
        if "--ask" in sys.argv[2:]:
            env["JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT"] = "ask"
        if "--background" in sys.argv[2:]:
            env["JCODE_DESKTOP_SCREENSHOT_SWARM"] = "1"
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
                if "--background" in sys.argv[2:]:
                    opened = request(endpoint, {"action": "open_session", "session_id": "swarm-fixture-0", "destination": {"background_panel": {"window_id": window}}})
                    assert opened["status"] == "ok", opened
                    background = {"window_id": window, "session_id": "swarm-fixture-0"}
                    state = request(endpoint, {"action": "state", "target": background})
                    assert state["status"] == "ok", state
                    protected = request(endpoint, {"action": "set_composer", "target": target, "text": "must not overwrite"})
                    assert protected["status"] == "error" and protected["code"] == "user_window_focused", protected
                    set_background = request(endpoint, {"action": "set_composer", "target": background, "text": "safe background draft"})
                    assert set_background["status"] == "ok", set_background
                    state = request(endpoint, {"action": "state", "target": background})
                    assert state["result"]["panel"]["composer"] == "safe background draft", state
                    print("private Desktop background session and focused composer protection passed")
                    return
                if "--ask" in sys.argv[2:]:
                    for _ in range(60):
                        state = request(endpoint, {"action": "state", "target": target})
                        ask = state["result"]["panel"]["ask"]
                        if ask and ask["option_selectors"]:
                            break
                        time.sleep(0.05)
                    assert ask and ask["option_selectors"], state
                    selector = ask["option_selectors"][1]
                    denied = request(endpoint, {"action": "click", "target": target, "selector": selector})
                    assert denied["status"] == "error", denied
                    clicked = request(endpoint, {"action": "click", "target": target, "selector": selector, "allow_user_window": True})
                    assert clicked["status"] == "ok", clicked
                    state = request(endpoint, {"action": "state", "target": target})
                    assert state["result"]["panel"]["ask"]["chosen"][0] == [1], state
                    print("private Desktop socket ask selection passed")
                    return
                for _ in range(60):
                    state = request(endpoint, {"action": "state", "target": target})
                    rows = state["result"]["panel"]["visible_transcript_items"] or []
                    selectors = [row["selector"] for row in rows if row["selector"]]
                    if selectors:
                        break
                    time.sleep(0.05)
                assert selectors, f"no visible fixture tool selector: {state['result']['panel']['visible_transcript_items']!r}, navigation={state['result']['navigation']!r}"
                wait = request(endpoint, {"action": "wait_for", "target": target, "condition": {"kind": "selector_visible", "selector": selectors[0]}, "timeout_ms": 1000})
                assert wait["status"] == "ok", wait
                blocked = []
                waiter = threading.Thread(target=lambda: blocked.append(request(endpoint, {"action": "wait_for", "target": target, "condition": {"kind": "text_contains", "text": "never-present-9381948"}, "timeout_ms": 1500})))
                waiter.start()
                time.sleep(0.1)
                start = time.monotonic()
                concurrent = request(endpoint, {"action": "state", "target": target})
                assert concurrent["status"] == "ok" and time.monotonic() - start < 0.8, concurrent
                waiter.join(timeout=3)
                assert not waiter.is_alive() and blocked[0]["status"] == "error", blocked
                element_png = root / "tool.png"
                element = request(endpoint, {"action": "render", "target": target, "selector": selectors[0], "path": str(element_png)})
                assert element["status"] == "ok", element
                assert element_png.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
                protected = request(endpoint, {"action": "set_composer", "target": target, "text": "must not overwrite"})
                assert protected["status"] == "error" and protected["code"] == "user_window_focused", protected
                result = request(endpoint, {"action": "set_composer", "target": target, "text": "isolated draft", "allow_user_window": True})
                assert result["status"] == "ok", result
                denied_send = request(endpoint, {"action": "send", "target": target})
                assert denied_send["status"] == "error" and denied_send["code"] == "user_window_focused", denied_send
                state = request(endpoint, {"action": "state", "target": target})
                assert state["result"]["panel"]["composer"] == "isolated draft", state
                sent = request(endpoint, {"action": "send", "target": target, "allow_user_window": True})
                assert sent["status"] == "ok" and sent["result"]["submitted"], sent
                state = request(endpoint, {"action": "state", "target": target})
                assert state["result"]["panel"]["composer"] == "", state
                for action in ({"action": "press_key", "key": "enter"}, {"action": "click", "selector": "composer.send"}):
                    result = request(endpoint, {"action": "set_composer", "target": target, "text": "isolated draft", "allow_user_window": True})
                    assert result["status"] == "ok", result
                    sent = request(endpoint, {**action, "target": target, "allow_user_window": True})
                    assert sent["status"] == "ok" and sent["result"]["submitted"], sent
                wrong = request(endpoint, {"action": "state", "target": {**target, "session_id": "wrong"}})
                assert wrong["status"] == "error", wrong
                search = request(endpoint, {"action": "search", "target": target, "query": "needle", "allow_user_window": False})
                assert search["status"] == "error", search
                search = request(endpoint, {"action": "search", "target": target, "query": "needle", "allow_user_window": True})
                assert search["status"] == "ok", search
                state = request(endpoint, {"action": "state", "target": target})
                assert state["result"]["search"]["query"] == "needle", state
                denied_escape = request(endpoint, {"action": "press_key", "target": target, "key": "escape"})
                assert denied_escape["status"] == "error", denied_escape
                escaped = request(endpoint, {"action": "press_key", "target": target, "key": "escape", "allow_user_window": True})
                assert escaped["status"] == "ok", escaped
                state = request(endpoint, {"action": "state", "target": target})
                assert not state["result"]["search_open"], state
                png = root / "window.png"
                rendered = request(endpoint, {"action": "render", "target": target, "selector": None, "path": str(png)})
                assert rendered["status"] == "ok", rendered
                assert png.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
                print("private Desktop socket list/state/set_composer/send/search/render passed")
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
