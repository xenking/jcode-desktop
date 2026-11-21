#!/usr/bin/env python3
"""Verify direct Soniox dictation in a hidden, isolated macOS debug Desktop.

Usage: python3 scripts/accept-soniox-rt.py DEBUG_BINARY NEW_EVIDENCE_DIR
Uses local Russian TTS as PCM input and a loopback Soniox server. No live STT,
physical microphone, application credentials, or Jcode daemon is used.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import shlex
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import wave

from screenshot import isolated_env

KEY = "desktop-loopback-fixture"
PHRASE = "Привет, проверяем диктовку."
PREFIX = "До "
SUFFIX = "после"


def native_request():
    spec = importlib.util.spec_from_file_location(
        "native_control", Path(__file__).with_name("accept-native-control.py")
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.request


def recorder(path):
    with wave.open(path, "rb") as audio:
        assert (audio.getnchannels(), audio.getsampwidth(), audio.getframerate()) == (1, 2, 16000)
        while True:
            chunk = audio.readframes(320) or bytes(640)
            sys.stdout.buffer.write(chunk)
            sys.stdout.buffer.flush()
            time.sleep(0.02)


def receive(stream, size):
    data = stream.read(size)
    if len(data) != size:
        raise EOFError("truncated WebSocket frame")
    return data


def frame(stream):
    first, second = receive(stream, 2)
    assert first & 0x80 and second & 0x80, "expected one masked client frame"
    size = second & 127
    if size >= 126:
        size = int.from_bytes(receive(stream, 2 if size == 126 else 8), "big")
    assert size <= 1 << 20
    mask = receive(stream, 4)
    data = receive(stream, size)
    return first & 15, bytes(value ^ mask[i % 4] for i, value in enumerate(data))


def publish(conn, tokens, finished=False):
    body = json.dumps({"tokens": [{"text": text, "is_final": final} for text, final in tokens],
                       "finished": finished}, ensure_ascii=False).encode()
    length = bytes([len(body)]) if len(body) < 126 else bytes([126]) + struct.pack("!H", len(body))
    stamp = time.monotonic_ns()
    conn.sendall(bytes([0x81]) + length + body)
    return stamp


def soniox(listener, expected_pcm, times, revise, failures):
    try:
        with listener:
            conn, _ = listener.accept()
            with conn, conn.makefile("rb") as stream:
                conn.settimeout(15)
                assert stream.readline() == b"GET /transcribe-websocket HTTP/1.1\r\n"
                headers = {}
                while (line := stream.readline()) != b"\r\n":
                    name, value = line.decode().split(":", 1)
                    headers[name.lower()] = value.strip()
                assert headers.get("authorization") == f"Bearer {KEY}", "fixture auth missing"
                magic = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
                accept = base64.b64encode(hashlib.sha1((headers["sec-websocket-key"] + magic).encode()).digest())
                conn.sendall(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\n"
                             b"Upgrade: websocket\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n")
                opcode, data = frame(stream)
                setup = json.loads(data)
                assert opcode == 1 and setup["model"] == "stt-rt-v6"
                assert setup["audio_format"] == "pcm_s16le" and setup["sample_rate"] == 16000
                assert setup["num_channels"] == 1 and "ru" in setup["language_hints"]
                assert "api_key" not in setup
                times["config_received"] = time.monotonic_ns()
                audio = bytearray()
                while True:
                    opcode, data = frame(stream)
                    if opcode == 1 and not data:
                        break
                    if opcode == 1:
                        assert json.loads(data) == {"type": "keepalive"}
                        continue
                    assert opcode == 2 and data and len(data) % 2 == 0
                    times.setdefault("first_pcm_received", time.monotonic_ns())
                    if len(audio) < len(expected_pcm):
                        audio.extend(data[:len(expected_pcm) - len(audio)])
                        if len(audio) == len(expected_pcm):
                            assert audio == expected_pcm, "PCM received differs from the Russian fixture"
                            times["phrase_pcm_received"] = time.monotonic_ns()
                    if "first_partial_sent" not in times and len(audio) >= 12800 and any(audio):
                        times["first_partial_sent"] = publish(conn, [("Привет", False)])
                    if revise.is_set() and "revision_sent" not in times:
                        times["revision_sent"] = publish(conn, [("Привет,", True), (" проверяю", False)])
                times["stop_frame_received"] = time.monotonic_ns()
                assert "revision_sent" in times, "recording stopped before the revision"
                times["final_sent"] = publish(conn, [(" проверяем", True), (" диктовку.", True), ("<fin>", True)])
                times["finished_sent"] = publish(conn, [], finished=True)
    except Exception as error:
        failures.append(error)


def wait(predicate, label, failures, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if failures:
            raise RuntimeError("Soniox fixture failed") from failures[0]
        if result := predicate():
            return result
        time.sleep(0.01)
    raise TimeoutError(f"timed out waiting for {label}")


def verify(binary, out, root):
    for name in ("home", "runtime", "config", "cache", "data", "logs", "jcode", "tmp"):
        (root / name).mkdir(mode=0o700)
    env = isolated_env(root)
    speech = root / "speech.wav"
    subprocess.run(["/usr/bin/say", "-v", "Milena", "-o", str(root / "speech.aiff"), PHRASE],
                   env=env, check=True, timeout=20)
    subprocess.run(["/usr/bin/afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1",
                    str(root / "speech.aiff"), str(speech)], env=env, check=True, timeout=10)
    command = "exec " + shlex.join([sys.executable, str(Path(__file__).resolve()), "--pcm", str(speech)])
    listener = socket.create_server(("127.0.0.1", 0))
    listener.settimeout(45)
    port = listener.getsockname()[1]
    (root / "jcode/config.toml").write_text(
        f'[dictation]\nbackend = "soniox"\nmode = "insert"\n'
        f'base_url = "ws://127.0.0.1:{port}/transcribe-websocket"\nrecorder = {json.dumps(command)}\n'
    )
    env.update(JCODE_TEST_SESSION="1", JCODE_VOICE_TIMING="1", SONIOX_API_KEY=KEY,
               JCODE_DESKTOP_CONTROL_TEST="1", JCODE_DESKTOP_CONTROL_DIR=str(root / "control"),
               JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT="soniox-rt", JCODE_DESKTOP_MUTE_SOUNDS="1",
               JCODE_DESKTOP_CONFIG=str(root / "desktop.toml"), JCODE_NO_TELEMETRY="1",
               JCODE_SOCKET=str(root / "runtime/unused.sock"),
               JCODE_API_SOCKET=str(root / "runtime/unused-api.sock"), TMPDIR=str(root / "tmp"))
    (root / "desktop.toml").write_text("[appearance]\nreduce_motion = true\n")
    times, failure = {}, []
    revise = threading.Event()
    with wave.open(str(speech), "rb") as source:
        expected_pcm = source.readframes(source.getnframes())
    server = threading.Thread(target=soniox, args=(listener, expected_pcm, times, revise, failure), daemon=True)
    server.start()
    request = native_request()
    with (out / "desktop.log").open("w") as log:
        process = subprocess.Popen([str(binary), "--single-panel", "--new-process", "--no-hot-reload"],
                                   env=env, cwd=root / "home", stdout=log, stderr=log)
        try:
            def endpoint():
                if process.poll() is not None:
                    raise RuntimeError(f"private Desktop exited with {process.returncode}")
                return next((root / "control").glob(f"{process.pid}-*.sock"), None)

            control = wait(endpoint, "private Desktop socket", failure)

            def call(action, **kwargs):
                reply = request(control, {"action": action, **kwargs})
                if reply.get("status") != "ok":
                    raise RuntimeError(f"native {action} failed: {reply}")
                return reply["result"]

            listing = call("list")
            window = listing["window_id"]
            assert window.startswith(f"{process.pid}:")
            sessions = [panel["session"] for row in listing["navigation"]["rows"] for panel in row["panels"]]
            assert sessions == ["screenshot-fixture"], sessions
            target = {"window_id": window, "session_id": sessions[0]}

            def drive(action, **kwargs):
                return call(action, target=target, allow_user_window=True, **kwargs)

            def state():
                panel = call("state", target=target)["panel"]
                return {"committed": panel["composer"], **panel["soniox_fixture"]}

            def capture(stage):
                path = out / f"{stage}.png"
                call("render", target=target, path=str(path))
                return hashlib.sha256(path.read_bytes()).hexdigest()

            def move(key, count):
                for _ in range(count):
                    drive("press_key", key=f"fixture:{key}")

            draft = PREFIX + SUFFIX
            drive("set_composer", text=draft)
            move("left", len(SUFFIX))
            assert state() == {"committed": draft, "text": draft,
                               "caret_byte": len(PREFIX.encode()), "provisional": False}
            times["start_click"] = time.monotonic_ns()
            drive("click", selector="composer.voice")

            def observe(stage, spoken, suffix_offset=0, final=False):
                left = f"{PREFIX}{spoken} "
                expected = {"committed": left + SUFFIX if final else draft, "text": left + SUFFIX,
                            "caret_byte": len((left + SUFFIX[:suffix_offset]).encode()),
                            "provisional": not final}
                try:
                    found = wait(lambda: (actual := state()) == expected and actual, stage, failure)
                except TimeoutError as error:
                    capture(f"failed-{stage}")
                    raise AssertionError(f"{stage}: expected {expected}, got {state()}; stages {list(times)}") from error
                times[stage] = time.monotonic_ns()
                report["states"][stage] = found
                report["screenshots"][stage] = capture(stage)

            report = {"states": {}, "screenshots": {}}
            observe("first_partial_visible", "Привет")
            move("right", 2)
            revise.set()
            observe("revision_visible", "Привет, проверяю", suffix_offset=2)
            wait(lambda: "phrase_pcm_received" in times, "complete Russian PCM at loopback server", failure)
            times["stop_click"] = time.monotonic_ns()
            drive("click", selector="composer.voice")
            observe("final_visible", PHRASE, suffix_offset=2, final=True)
            server.join(timeout=3)
            assert not server.is_alive() and not failure, failure
            time.sleep(0.15)
            assert state() == report["states"]["final_visible"], "late duplicate final"
            assert len(set(report["screenshots"].values())) == 3
            assert times["first_partial_visible"] < times["revision_visible"] < times["stop_click"]
            assert times["first_partial_visible"] < times["phrase_pcm_received"] <= times["stop_click"]
            assert times["stop_click"] < times["stop_frame_received"] <= times["final_sent"] < times["final_visible"]
            return {**report, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "version": subprocess.check_output([str(binary), "--version"], env=env, text=True).strip(),
                    "clock": "monotonic_ns, same host; milliseconds relative to start_click",
                    "relative_to_start_ms": {label: round((stamp - times["start_click"]) / 1e6, 2)
                                             for label, stamp in times.items()},
                    "deltas_ms": {f"{end} minus {start}": round((times[end] - times[start]) / 1e6, 2)
                                  for start, end in [
                                      ("first_pcm_received", "first_partial_visible"),
                                      ("first_partial_sent", "first_partial_visible"),
                                      ("stop_click", "first_partial_visible"),
                                      ("stop_click", "final_visible"),
                                  ]}}
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("requires macOS hidden Desktop and local Milena voice")
    out = args.output.resolve()
    out.mkdir(mode=0o700)
    with tempfile.TemporaryDirectory(prefix="jsx-", dir="/private/tmp") as profile:
        result = verify(args.binary.resolve(strict=True), out, Path(profile))
    print(json.dumps(result, ensure_ascii=False, indent=2))
    (out / "proof.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--pcm":
        recorder(sys.argv[2])
    else:
        main()
