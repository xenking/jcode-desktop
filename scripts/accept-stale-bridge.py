#!/usr/bin/env python3
"""Exercise real Desktop bridge selection against a copied legacy orphan.

The Desktop harness test executable is copied into a private app bundle beside
its new CLI. All processes use a private HOME/runtime and a local fake provider.
No installed application, daemon, configuration or session is read or changed.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jcode', type=Path, required=True)
    parser.add_argument('--old-jcode', type=Path, required=True)
    parser.add_argument('--harness-test', type=Path, required=True)
    parser.add_argument('--jcode-repo', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location('recovery_fixture', args.jcode_repo / 'tests/e2e/reload_recovery_owner.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    class BridgeFixture(module.Fixture):
        def start(self):
            self.daemon = subprocess.Popen([self.binary, '--no-update', '--no-selfdev',
                '--provider', 'openai-compatible', '--model', 'fake-model',
                'serve', '--socket', str(self.path)], env=self.env, cwd=self.root,
                stdout=self.log, stderr=self.log, start_new_session=True)
            module.eventually('isolated daemon socket', lambda: self.path.exists())
    fixture = BridgeFixture(args.jcode.resolve(), 'stale-bridge')
    desktop = None
    idle = None
    old_pid = None
    identity = None
    transcript = []
    evidence = {'passed': False}
    try:
        old_bundle = fixture.root / 'Old.app/Contents/MacOS'
        new_bundle = fixture.root / 'New.app/Contents/MacOS'
        old_bundle.mkdir(parents=True)
        new_bundle.mkdir(parents=True)
        wrong_bundle = fixture.root / 'Wrong.app/Contents/MacOS'
        wrong_bundle.mkdir(parents=True)
        shutil.copy2(args.jcode, wrong_bundle / 'jcode')
        shutil.copy2(args.old_jcode, old_bundle / 'jcode')
        shutil.copy2(args.jcode, new_bundle / 'jcode')
        shutil.copy2(args.harness_test, new_bundle / 'jcode-desktop')
        api = fixture.runtime / 'jcode-api.sock'
        env = fixture.env | {'JCODE_API_SOCKET': str(api), 'JCODE_TEST_SESSION': '1'}
        old_log = open(args.output / 'old-bridge.log', 'w')
        wrapper = subprocess.run([sys.executable, '-c',
            'import os,subprocess,sys,time; p=subprocess.Popen([sys.argv[1],"--no-update","--no-selfdev","api-bridge"],stdout=sys.stderr,stderr=sys.stderr); print(p.pid,flush=True); time.sleep(.5); os._exit(0)',
            str(old_bundle / 'jcode')], env=env, cwd=fixture.root,
            stdout=subprocess.PIPE, stderr=old_log, text=True, timeout=10)
        old_pid = int(wrapper.stdout.strip())
        module.eventually('legacy orphan socket', lambda: api.exists())
        parent = subprocess.check_output(['ps', '-p', str(old_pid), '-o', 'ppid='], text=True).strip()
        assert parent == '1', f'copied old bridge must be orphaned, parent={parent}'
        old = module.Wire(api)
        old.send({'v': 1, 'id': 1, 'req': 'hello', 'client': 'stale-bridge-old-probe', 'min_version': 1, 'max_version': 1})
        old_hello = old.event(lambda e: e.get('ev') == 'hello_ok')
        old.close()
        assert 'bridge_lifecycle_v1' not in old_hello.get('capabilities', []), 'fixture must use a genuinely old bridge'
        peer, session = fixture.connect()
        env |= {'JCODE_STALE_BRIDGE_SESSION': session,
                'JCODE_STALE_BRIDGE_EXPECTED_BINARY': str(new_bundle / 'jcode'),
                'JCODE_STALE_BRIDGE_WRONG_BINARY': str(wrong_bundle / 'jcode')}
        desktop = subprocess.Popen([str(new_bundle / 'jcode-desktop'),
            'real_bundle_bridge_replaces_stale_bridge_and_delivers_peer_prompt_live',
            '--ignored', '--nocapture', '--test-threads=1'], env=env,
            cwd=fixture.root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        buffer = b''
        sent = False
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            if not select.select([desktop.stdout], [], [], .1)[0]:
                continue
            data = os.read(desktop.stdout.fileno(), 65536)
            if not data:
                break
            buffer += data
            while b'\n' in buffer:
                raw, buffer = buffer.split(b'\n', 1)
                line = raw.decode(errors='replace')
                transcript.append(line)
                if 'BUNDLE_BRIDGE_IDENTITY ' in line:
                    identity = json.loads(line.split('BUNDLE_BRIDGE_IDENTITY ', 1)[1])
                if 'BUNDLE_BRIDGE_DESKTOP_ATTACHED' in line and not sent:
                    assert identity and identity['pid'] != old_pid
                    assert identity['binary_path'] == str(new_bundle / 'jcode')
                    os.kill(old_pid, 0)
                    peer.send({'type': 'message', 'id': 2, 'content': 'stale-bridge-peer-prompt'})
                    fixture.release.set()
                    sent = True
                    terminal = peer.event(lambda event: event.get('type') in ('done', 'error') and event.get('id') == 2, 15)
                    (args.output / 'peer-events.json').write_text(json.dumps(peer.seen, indent=2) + '\n')
                    assert terminal['type'] != 'error', terminal
            if desktop.poll() is not None:
                break
        status = desktop.wait(timeout=5)
        assert status == 0, '\n'.join(transcript)
        assert sent and any('BUNDLE_BRIDGE_PEER_PROMPT_LIVE' in line for line in transcript), '\n'.join(transcript)
        assert identity['pid'] != old_pid
        def new_bridge_gone():
            try:
                os.kill(identity['pid'], 0)
                return False
            except ProcessLookupError:
                return True
        module.eventually('new bridge exits after Desktop parent dies', new_bridge_gone, 10)
        os.kill(old_pid, 0)
        idle_socket = fixture.runtime / 'idle.sock'
        idle_started = time.monotonic()
        idle = subprocess.Popen([str(new_bundle / 'jcode'), '--no-update', '--no-selfdev', 'api-bridge'],
            env=env | {'JCODE_API_SOCKET': str(idle_socket)}, cwd=fixture.root,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        module.eventually('idle bridge listener', lambda: idle_socket.exists())
        assert idle.wait(timeout=35) == 0
        idle_seconds = time.monotonic() - idle_started
        assert 29 <= idle_seconds <= 35, idle_seconds
        assert not idle_socket.exists(), 'idle bridge left its listener behind'
        evidence |= {'passed': True, 'old_pid': old_pid, 'old_ppid': int(parent),
                     'old_server': old_hello.get('server'), 'new_identity': identity,
                     'peer_prompt_live': True, 'new_bridge_reaped_after_parent_exit': True,
                     'idle_bridge_exit_seconds': round(idle_seconds, 3),
                     'private_home': str(fixture.home), 'private_runtime': str(fixture.runtime)}
    finally:
        if idle and idle.poll() is None:
            idle.terminate()
            idle.wait(timeout=5)
        if desktop and desktop.poll() is None:
            desktop.terminate()
            desktop.wait(timeout=5)
        if old_pid:
            # Only the PID returned by the private wrapper, never a global scan.
            try:
                os.kill(old_pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        (args.output / 'desktop.log').write_text('\n'.join(transcript) + '\n')
        shutil.copy2(fixture.root / 'daemon.log', args.output / 'daemon.log')
        if (fixture.home / 'logs').exists():
            shutil.copytree(fixture.home / 'logs', args.output / 'logs', dirs_exist_ok=True)
        fixture.close()
        evidence['private_fixture_removed'] = not fixture.root.exists()
        (args.output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    main()
