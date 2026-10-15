"""Exercise the production POSIX scripts without Python on the remote PATH."""
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import unittest
import uuid

SCRIPTS = Path(__file__).resolve().parents[1] / "src-tauri/src/transfers/direct/scripts"
GUARD = "00000000-0000-0000-0000-000000000000"

class ShellTransferTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "任务 ' space\n"
        self.root.mkdir()
        self.bin = Path(self.temp.name) / "bin"
        self.bin.mkdir()
        for tool in ["sh", "cat", "sleep", "mkdir", "rmdir", "mv", "ps", "ssh"]:
            (self.bin / tool).symlink_to(shutil.which(tool))
        self.environment = dict(os.environ, PATH=str(self.bin))
        self.run_id = str(uuid.uuid4())
        (self.root / "run").write_text(self.run_id + "\n")
        (self.root / "lease").write_text("0\n")
        for name in ["supervisor.sh", "sender.sh", "receiver.sh"]:
            shutil.copy(SCRIPTS / name, self.root / name)
        self.children = []
        self.stop = threading.Event()
        def heartbeat():
            value = 0
            while not self.stop.wait(.2):
                value += 1
                path = self.root / "lease.next"
                path.write_text(str(value) + "\n")
                path.replace(self.root / "lease")
        self.beater = threading.Thread(target=heartbeat)
        self.beater.start()
        self.guard = self.launch("guard", GUARD)
        self.wait_for(lambda: (self.root / "guard.state").exists())

    def tearDown(self):
        self.stop.set()
        self.beater.join()
        for child in self.children:
            if child.poll() is None:
                try: os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError: pass
            child.wait()
            if child.stderr: child.stderr.close()
        self.temp.cleanup()

    def wait_for(self, predicate, timeout=8):
        start = time.monotonic()
        while not predicate():
            if time.monotonic() - start > timeout: self.fail("timed out")
            time.sleep(.02)

    def launch(self, mode, attempt, input_file=None):
        child = subprocess.Popen(["/bin/sh", str(self.root / "supervisor.sh"), mode,
                                  str(self.root), self.run_id, attempt],
                                 stdin=input_file or subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.PIPE, start_new_session=True, env=self.environment)
        self.children.append(child)
        return child

    def job(self, command):
        attempt = str(uuid.uuid4())
        job = self.root / ("job-" + attempt)
        job.mkdir()
        (job / "command.sh").write_text(command + "\n")
        return attempt, job

    def seal(self):
        result = self.launch("seal", GUARD)
        self.assertEqual(result.wait(timeout=8), 0, result.stderr.read())
        self.assertEqual(self.guard.wait(timeout=8), 0)

    def test_binary_and_special_destination(self):
        for tool in ["python", "python3", "scp", "rsync"]:
            self.assertIsNone(shutil.which(tool, path=self.environment["PATH"]))
        source = self.root / "source"
        source.write_bytes(bytes(range(256)) * 1000)
        target = self.root / "目标 ' $(bad)\\\n"
        attempt, job = self.job("exec sh " + shlex.quote(str(self.root / "receiver.sh")) + " " + shlex.quote(str(target)))
        with source.open("rb") as stream:
            child = self.launch("run", attempt, stream)
            self.assertEqual(child.wait(timeout=8), 0, child.stderr.read())
        self.assertEqual(source.read_bytes(), target.read_bytes())
        self.assertIn("done 0", (job / "state").read_text())
        self.seal()

    def test_exclusive_create_does_not_replace(self):
        target = self.root / "target"
        target.write_text("original")
        attempt, job = self.job("exec sh " + shlex.quote(str(self.root / "receiver.sh")) + " " + shlex.quote(str(target)))
        child = self.launch("run", attempt)
        self.assertEqual(child.wait(timeout=8), 73)
        self.assertEqual(target.read_text(), "original")
        self.assertIn("failed 73", (job / "state").read_text())

    def test_seal_stops_owned_process_and_rejects_late_start(self):
        attempt, job = self.job("exec sleep 120")
        child = self.launch("run", attempt)
        self.wait_for(lambda: (job / "admitted").exists())
        self.seal()
        self.assertEqual(child.wait(timeout=8), 130, child.stderr.read())
        self.assertIn("cancelled 130", (job / "state").read_text())
        late, late_job = self.job("exit 0")
        self.assertEqual(self.launch("run", late).wait(timeout=8), 75)
        self.assertFalse((late_job / "admitted").exists())

    def test_same_attempt_cannot_start_twice(self):
        attempt, job = self.job("exit 0")
        self.assertEqual(self.launch("run", attempt).wait(timeout=8), 0)
        self.assertEqual(self.launch("run", attempt).wait(timeout=8), 75)
        self.assertIn("done 0", (job / "state").read_text())

    def test_lost_heartbeat_closes_admission_and_stops_writer(self):
        attempt, job = self.job("exec sleep 120")
        child = self.launch("run", attempt)
        self.wait_for(lambda: (job / "admitted").exists())
        self.stop.set()
        self.beater.join()
        self.assertEqual(child.wait(timeout=38), 130)
        self.assertEqual(self.guard.wait(timeout=8), 0)
        self.assertIn("closed 0", (self.root / "guard.state").read_text())
        late, _ = self.job("exit 0")
        self.assertEqual(self.launch("run", late).wait(timeout=8), 75)

    def test_stubborn_child_never_gets_a_false_stop_receipt(self):
        ready = self.root / "stubborn-ready"
        inner = "trap '' HUP INT TERM; : > " + shlex.quote(str(ready)) + "; while :; do sleep 1; done"
        attempt, job = self.job("exec sh -c " + shlex.quote(inner))
        child = self.launch("run", attempt)
        self.wait_for(ready.exists)
        self.seal()
        self.assertEqual(child.wait(timeout=8), -signal.SIGKILL)
        self.assertIn("running 0", (job / "state").read_text())

    def test_nonisolated_duplicate_cannot_overwrite_running_receipt(self):
        attempt, job = self.job("exec sleep 120")
        child = self.launch("run", attempt)
        self.wait_for(lambda: (job / "state").exists())
        rejected = subprocess.run(["/bin/sh", str(self.root / "supervisor.sh"), "run", str(self.root), self.run_id, attempt], capture_output=True, env=self.environment)
        self.assertEqual(rejected.returncode, 69)
        self.assertIn("running 0", (job / "state").read_text())
        self.seal()
        self.assertEqual(child.wait(timeout=8), 130)

if __name__ == "__main__": unittest.main()
