"""Supervisor protocol tests; fake scp only exercises process ownership and output bounds."""
import importlib.util
import json
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import tempfile
import threading
import unittest

RUNNER = Path(__file__).resolve().parents[1] / "src-tauri/src/transfers/remote_runner.py"


class SupervisorTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        (self.root / "config.json").write_text(json.dumps(dict(runId="test-run", host="127.0.0.1", port=22, user="fixture", publicKey="ssh-ed25519 fixture")))
        (self.root / "source").write_bytes(b"content")
        (self.bin / "ssh").write_text("#!/bin/sh\necho OpenSSH_9.0 >&2\n")
        (self.bin / "ssh").chmod(0o700)
        self.process = None

    def tearDown(self):
        if self.process:
            if self.process.poll() is None:
                self.process.stdin.close()
                self.process.wait(timeout=8)
            self.process.stdout.close()
            self.process.stderr.close()
            if not self.process.stdin.closed:
                self.process.stdin.close()
        self.temporary.cleanup()

    def start(self, flood=False):
        code = ("import sys\nsys.stderr.write('error detail ' * 200000)\nsys.exit(1)\n" if flood else """
import os, signal, subprocess, sys, time
from pathlib import Path
child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'])
Path(os.environ['FIXTURE_PID']).write_text(str(child.pid))
def stop(*_):
    child.terminate()
    child.wait(timeout=5)
    sys.exit(1)
signal.signal(signal.SIGTERM, stop)
while True: time.sleep(.1)
""")
        scp = self.bin / "scp"
        scp.write_text("#!" + sys.executable + "\n" + code)
        scp.chmod(0o700)
        launcher = "import importlib.util,sys; s=importlib.util.spec_from_file_location('runner',sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m); m.LEASE=3; m.Runner(sys.argv[2]).run()"
        self.process = subprocess.Popen([sys.executable, "-u", "-c", launcher, str(RUNNER), str(self.root)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"], FIXTURE_PID=str(self.root / "child.pid")))
        self.messages = queue.Queue()
        def collect():
            for line in self.process.stdout:
                self.messages.put(json.loads(line))
        threading.Thread(target=collect, daemon=True).start()
        self.assertEqual(self.messages.get(timeout=5)["type"], "ready")

    def send(self, kind, **values):
        self.process.stdin.write((json.dumps(dict(runId="test-run", type=kind, **values)) + "\n").encode())
        self.process.stdin.flush()

    def copy(self):
        self.send("copy", itemId=1, source=str(self.root / "source"), destination="/unused", size=7, mtime=None)
        self.assertEqual(self.messages.get(timeout=5)["type"], "running")

    def assert_stopped(self):
        self.assertEqual(self.messages.get(timeout=8)["type"], "cancelled")
        self.process.wait(timeout=5)
        self.assertEqual(json.loads((self.root / "state.json").read_text())["status"], "cancelled")
        pid = self.root / "child.pid"
        if pid.exists():
            with self.assertRaises(ProcessLookupError): os.kill(int(pid.read_text()), 0)

    def test_lease_expiry_stops_process_group(self):
        self.start()
        self.copy()
        self.assert_stopped()

    def test_eof_stops_process_group(self):
        self.start()
        self.copy()
        self.process.stdin.close()
        self.assert_stopped()

    def test_cancel_ack_follows_process_exit(self):
        self.start()
        self.copy()
        self.send("cancel")
        self.assert_stopped()

    def test_ssh_hangup_stops_process_group(self):
        self.start()
        self.copy()
        self.process.send_signal(signal.SIGHUP)
        self.assert_stopped()

    def test_stderr_is_bounded_and_does_not_block_control(self):
        self.start(flood=True)
        self.copy()
        failed = self.messages.get(timeout=5)
        self.assertEqual(failed["type"], "itemFailed")
        self.assertLessEqual(len(failed["error"]), 16384)
        self.send("finish")
        self.assertEqual(self.messages.get(timeout=5)["type"], "finished")
        self.process.wait(timeout=5)
        self.assertFalse((self.root / "stderr").exists())

if __name__ == "__main__": unittest.main()
