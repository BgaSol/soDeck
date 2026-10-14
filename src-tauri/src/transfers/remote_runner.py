"""Per-task supervisor on the source host. File contents only flow scp -> target.

stdin/stdout are bounded JSON-lines control messages; stderr from children never
shares the protocol. A lost controller lease stops the entire child process group.
"""
import json
import os
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import time

LEASE = 30.0
MAX_LINE = 262144


class Runner:
    def __init__(self, root):
        self.root = root
        self.config = json.load(open(os.path.join(root, "config.json"), encoding="utf-8"))
        self.run_id = self.config["runId"]
        self.selector = selectors.DefaultSelector()
        self.selector.register(sys.stdin.buffer, selectors.EVENT_READ, "control")
        self.buffer = b""
        self.last_ping = time.monotonic()
        self.child = None
        self.current = None
        self.status = {"runId": self.run_id, "status": "starting", "itemId": None}
        self.ssh = shutil.which("ssh")
        self.scp = shutil.which("scp")
        self.closed = False
        self.errors = bytearray()
        # sshd may deliver HUP/TERM before stdin reports EOF on disconnect.
        # Let the supervisor reap scp before exiting in either case.
        signal.signal(signal.SIGHUP, self.disconnected)
        signal.signal(signal.SIGTERM, self.disconnected)

    def disconnected(self, *_):
        self.closed = True

    def report(self, kind, **values):
        event = dict(runId=self.run_id, type=kind, **values)
        self.status.update(event)
        self.status["status"] = kind
        temporary = os.path.join(self.root, "state.next")
        with open(temporary, "w", encoding="utf-8") as output:
            json.dump(self.status, output, ensure_ascii=True)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, os.path.join(self.root, "state.json"))
        try:
            print(json.dumps(event, ensure_ascii=True), flush=True)
        except (BrokenPipeError, OSError):
            self.closed = True

    def stop(self):
        child = self.child
        if child is None:
            return
        # OpenSSH scp's TERM handler terminates and waits for its ssh child.
        # If it does not exit, keep the unreaped leader until KILL is sent so
        # a recycled PID cannot be mistaken for this process group.
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        except PermissionError:
            if child.poll() is None:
                raise
        try:
            child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=5)
        self.close_errors()
        self.child = None

    def close_errors(self):
        if self.child is not None and self.child.stderr is not None:
            try:
                self.selector.unregister(self.child.stderr)
            except KeyError:
                pass
            self.child.stderr.close()

    def read_errors(self):
        data = os.read(self.child.stderr.fileno(), 65536)
        if not data:
            self.selector.unregister(self.child.stderr)
        self.errors.extend(data[:max(0, 16384 - len(self.errors))])
        return data

    def options(self):
        c = self.config
        known_hosts = os.path.join(self.root, "known_hosts").replace("%", "%%").replace("\\", "\\\\").replace('"', '\\"')
        values = {
            "BatchMode": "yes", "StrictHostKeyChecking": "yes",
            "UserKnownHostsFile": '"' + known_hosts + '"',
            "GlobalKnownHostsFile": "/dev/null", "KnownHostsCommand": "none",
            "HostKeyAlias": "ssh-workspace-target", "UpdateHostKeys": "no",
            "ProxyCommand": "none", "ProxyJump": "none", "ControlMaster": "no",
            "ControlPath": "none", "ClearAllForwardings": "yes", "ForwardAgent": "no",
            "PermitLocalCommand": "no", "RemoteCommand": "none", "RequestTTY": "no",
            "ForkAfterAuthentication": "no",
            "CanonicalizeHostname": "no", "Hostname": c["host"], "User": c["user"],
            "ConnectTimeout": "12", "ConnectionAttempts": "1",
            "ServerAliveInterval": "5", "ServerAliveCountMax": "3",
        }
        algorithm = c["publicKey"].split(" ", 1)[0]
        values["HostKeyAlgorithms"] = "rsa-sha2-512,rsa-sha2-256" if algorithm == "ssh-rsa" else algorithm
        result = []
        for key, value in values.items():
            result.extend(["-o", key + "=" + value])
        return result

    def start(self, message):
        if self.child is not None:
            raise ValueError("上一项目尚未完成")
        path = message["source"]
        before = os.lstat(path)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError("源项目不是普通文件，或已经变为符号链接")
        if before.st_size != message["size"] or (message.get("mtime") is not None and int(before.st_mtime) != message["mtime"]):
            raise ValueError("扫描后源文件发生变化，请重新创建任务")
        host = self.config["host"]
        if ":" in host:
            host = "[" + host + "]"
        target = host + ":" + message["destination"]
        self.current = (message, before)
        self.errors.clear()
        self.child = subprocess.Popen(
            [self.scp, "-s", "-B", "-S", self.ssh, "-P", str(self.config["port"])] + self.options() + ["--", path, target],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE, start_new_session=True,
        )
        os.set_blocking(self.child.stderr.fileno(), False)
        self.selector.register(self.child.stderr, selectors.EVENT_READ, "stderr")
        self.report("running", itemId=message["itemId"])

    def run(self):
        if os.name != "posix" or not self.ssh or not self.scp:
            raise ValueError("直传源端需要 POSIX、Python 3、OpenSSH ssh/scp")
        version = subprocess.run([self.ssh, "-V"], capture_output=True, timeout=5)
        found = re.search(rb"OpenSSH_(\d+)\.", version.stderr + version.stdout)
        if not found or int(found.group(1)) < 9:
            raise ValueError("直传源端需要 OpenSSH 9.0 或更新版本（SFTP 模式）")
        with open(os.path.join(self.root, "probe"), "wb"):
            pass
        self.last_ping = time.monotonic()
        self.report("ready")
        while True:
            if self.closed or time.monotonic() - self.last_ping > LEASE:
                self.stop()
                self.report("cancelled", reason="控制连接中断或心跳超时")
                return
            for key, _ in self.selector.select(0.1):
                if key.data == "stderr":
                    self.read_errors()
                    continue
                chunk = os.read(sys.stdin.fileno(), 65536)
                if not chunk:
                    self.closed = True
                    break
                self.buffer += chunk
                if len(self.buffer) > MAX_LINE:
                    raise ValueError("控制消息过大")
                while b"\n" in self.buffer:
                    line, self.buffer = self.buffer.split(b"\n", 1)
                    message = json.loads(line)
                    if message.get("runId") != self.run_id:
                        raise ValueError("任务代次不匹配")
                    self.last_ping = time.monotonic()
                    kind = message["type"]
                    if kind == "cancel":
                        self.stop()
                        self.report("cancelled")
                        return
                    if kind == "finish":
                        if self.child is not None:
                            raise ValueError("传输仍在运行")
                        self.report("finished")
                        return
                    if kind == "copy":
                        try:
                            self.start(message)
                        except (OSError, ValueError) as error:
                            self.report("itemFailed", itemId=message["itemId"], error=str(error))
                    elif kind != "ping":
                        raise ValueError("未知控制命令")
            if self.child is not None and self.child.poll() is not None:
                code = self.child.wait()
                try:
                    while self.read_errors():
                        pass
                except (BlockingIOError, KeyError):
                    pass
                self.close_errors()
                self.child = None
                error = self.errors.decode("utf-8", "replace")
                message, before = self.current
                try:
                    after = os.lstat(message["source"])
                    unchanged = stat.S_ISREG(after.st_mode) and (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
                except OSError:
                    unchanged = False
                if code == 0 and unchanged:
                    self.report("itemDone", itemId=message["itemId"], size=before.st_size)
                else:
                    self.report("itemFailed", itemId=message["itemId"], error=error.strip() or ("传输期间源文件变化，未提交目标文件" if not unchanged else "scp 命令失败（退出码 %s）" % code))


if __name__ == "__main__":
    runner = None
    try:
        runner = Runner(os.path.abspath(sys.argv[1]))
        runner.run()
    except BaseException as error:
        if runner:
            runner.stop()
            runner.report("failed", error=str(error))
        else:
            print(json.dumps({"type": "failed", "error": str(error)}), flush=True)
        sys.exit(1)
