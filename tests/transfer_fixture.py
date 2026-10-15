"""Isolated loopback SSH endpoints for transfer integration tests.

Both use SSH/SFTP and permit bounded capability checks and the bundled POSIX
task scripts under their fixture roots. A invokes real system OpenSSH ssh to B.
No arbitrary shell commands or paths outside the temporary roots are accepted.
"""
import argparse
import errno
import json
import os
from pathlib import Path
import shlex
import signal
import socket
import stat
import struct
import subprocess
import sys
import threading
import time
import paramiko

parser = argparse.ArgumentParser()
parser.add_argument("--root", required=True)
parser.add_argument("--port-a", type=int, required=True)
parser.add_argument("--port-b", type=int, required=True)
args = parser.parse_args()
root = Path(args.root).resolve()
root.mkdir(parents=True, exist_ok=True)
lock = threading.Lock()
children = set()
children_lock = threading.Lock()

def log(**event):
    with lock:
        with (root / "events.jsonl").open("a") as stream:
            stream.write(json.dumps(event) + "\n")

key_path = root / "identity"
identity = paramiko.RSAKey.generate(2048)
identity.write_private_key_file(str(key_path))
ssh_config = root / "ssh_config"
# Deliberately conflicting defaults verify that the production command pins
# the destination and disables proxies, reuse, interactive sessions and DNS
# host-key trust while still using A's own authentication configuration.
ssh_config.write_text("Host *\n  IdentityFile " + str(key_path) + "\n" + """  ProxyCommand /usr/bin/false
  ProxyJump invalid.invalid
  ControlMaster auto
  ControlPath /tmp/ssh-workspace-unused-fixture-socket
  RequestTTY force
  RemoteCommand false
  StdinNull yes
  SessionType none
  ForkAfterAuthentication yes
  ForwardAgent yes
  StrictHostKeyChecking no
  VerifyHostKeyDNS yes
  UserKnownHostsFile /dev/null
""")
wrappers = root / "bin"
wrappers.mkdir(exist_ok=True)
# Only the fixture source process receives this PATH. Existing user SSH config
# and keys are not modified. -F applies before production-enforced -o options.
(wrappers / "ssh").write_text("#!/bin/sh\nif [ -f " + shlex.quote(str(root / "deny-direct")) + " ] && [ \"$1\" != '-V' ]; then echo 'fixture: A cannot reach B' >&2; exit 255; fi\nexec /usr/bin/ssh -F " + shlex.quote(str(ssh_config)) + ' "$@"\n')
(wrappers / "ssh").chmod(0o700)
# Test-only receiver: exercises the production shell around a slow/failing cat.
# An absolute interpreter is used; the production scripts never invoke Python.
(wrappers / "cat").write_text("#!" + sys.executable + "\n" + r'''import json, os, pathlib, sys, time
root = pathlib.Path(os.environ["TRANSFER_FIXTURE_ROOT"])
while True:
    data = os.read(0, 32768)
    if not data: break
    if (root / "stderr-flood").exists():
        sys.stderr.write("receiver failed " * 131072)
        sys.stderr.flush()
        sys.exit(1)
    delay = root / "delay-write"
    if delay.exists(): time.sleep(float(delay.read_text()))
    if (root / "disk-full").exists():
        print("No space left on device", file=sys.stderr); sys.exit(1)
    with (root / "events.jsonl").open("a") as out:
        out.write(json.dumps(dict(event="write", side="b", bytes=len(data))) + "\n")
    try:
        offset = 0
        while offset < len(data): offset += os.write(1, data[offset:])
    except OSError as error:
        print(error, file=sys.stderr); sys.exit(1)
''')
(wrappers / "cat").chmod(0o700)


class Handle(paramiko.SFTPHandle):
    def __init__(self, flags, path, side):
        super().__init__(flags)
        self.path, self.side = path, side
    def stat(self):
        try: return paramiko.SFTPAttributes.from_stat(os.fstat((getattr(self, "readfile", None) or self.writefile).fileno()))
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def chattr(self, attr):
        try:
            stream = getattr(self, "writefile", None) or self.readfile
            if attr.st_size is not None: stream.truncate(attr.st_size)
            if attr.st_mode is not None: os.fchmod(stream.fileno(), attr.st_mode)
            return paramiko.SFTP_OK
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def read(self, offset, length):
        value = super().read(offset, length)
        log(event="read", side=self.side, path=str(self.path), bytes=len(value) if isinstance(value, bytes) else 0)
        return value
    def write(self, offset, data):
        if self.side == "b" and (root / "delay-write").exists():
            time.sleep(float((root / "delay-write").read_text()))
        log(event="write", side=self.side, path=str(self.path), bytes=len(data))
        return super().write(offset, data)

class Files(paramiko.SFTPServerInterface):
    def __init__(self, server, *values, side, **kwargs):
        super().__init__(server, *values, **kwargs)
        self.side = side
        self.owner = server
        self.base = root / side
    def path(self, value, parent=False, metadata=False):
        path = Path(value)
        if not path.is_absolute():
            path = self.base / path
        checked = path.parent.resolve() / path.name if parent else path.resolve()
        if checked != self.base and self.base not in checked.parents and not (metadata and checked in self.base.parents):
            raise PermissionError(errno.EACCES, "outside fixture")
        return checked
    def canonicalize(self, path):
        try: return str(self.path(path))
        except OSError: return str(self.base / "denied")
    def stat(self, path):
        try: return paramiko.SFTPAttributes.from_stat(os.stat(self.path(path, metadata=True)))
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def lstat(self, path):
        try: return paramiko.SFTPAttributes.from_stat(os.lstat(self.path(path, True, metadata=True)))
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def list_folder(self, path):
        try:
            result = []
            for child in self.path(path).iterdir():
                attr = paramiko.SFTPAttributes.from_stat(child.lstat()); attr.filename = child.name; result.append(attr)
            return result
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def chattr(self, path, attr):
        try: paramiko.SFTPServer.set_file_attr(str(self.path(path)), attr); return paramiko.SFTP_OK
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def open(self, path, flags, attr):
        try:
            path = self.path(path, True)
            fd = os.open(path, flags, attr.st_mode if attr.st_mode is not None else 0o644)
            handle = Handle(flags, path, self.side)
            if flags & os.O_RDWR:
                stream = os.fdopen(fd, "r+b", buffering=0); handle.readfile = stream; handle.writefile = stream
            elif flags & os.O_WRONLY:
                handle.writefile = os.fdopen(fd, "wb", buffering=0)
            else: handle.readfile = os.fdopen(fd, "rb", buffering=0)
            return handle
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def mkdir(self, path, attr):
        try: self.path(path, True).mkdir(mode=attr.st_mode or 0o755); return paramiko.SFTP_OK
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def rmdir(self, path):
        try:
            path = self.path(path, True)
            if self.side == "b" and path.name.startswith(".ssh-workspace-") and (root / "deny-cleanup").exists(): return paramiko.SFTP_PERMISSION_DENIED
            path.rmdir(); return paramiko.SFTP_OK
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def remove(self, path):
        try: self.path(path, True).unlink(); return paramiko.SFTP_OK
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def rename(self, source, target):
        try:
            source, target = self.path(source, True), self.path(target, True)
            os.link(source, target); source.unlink(); self.after_commit(target); return paramiko.SFTP_OK
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def posix_rename(self, source, target):
        try:
            target = self.path(target, True)
            os.replace(self.path(source, True), target); self.after_commit(target)
            if (root / "commit-connection-lost").exists(): return paramiko.SFTP_CONNECTION_LOST
            return paramiko.SFTP_OK
        except OSError as e: return paramiko.SFTPServer.convert_errno(e.errno)
    def after_commit(self, target):
        if self.side != "b": return
        if (root / "drop-commit-ack").exists() and target.name == "file":
            (root / "drop-commit-ack").unlink()
            self.owner.transport.close()
        if (root / "deny-after-first").exists() and target.name == "first":
            (root / "deny-direct").touch()

class Server(paramiko.ServerInterface):
    def __init__(self, side): self.side = side
    def get_allowed_auths(self, user): return "password,publickey"
    def check_auth_password(self, user, password):
        return paramiko.AUTH_SUCCESSFUL if user == "ops" and password == "fixture-password" else paramiko.AUTH_FAILED
    def check_auth_publickey(self, user, key):
        ok = user == "ops" and key == identity
        log(event="authentication", side=self.side, publickey=True, accepted=ok)
        return paramiko.AUTH_SUCCESSFUL if ok else paramiko.AUTH_FAILED
    def check_channel_request(self, kind, _): return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    def check_channel_exec_request(self, channel, command):
        try:
            values = shlex.split(command.decode())
            if not values or values[0] != "exec": return False
            values = values[1:]
            directory = None
            if len(values) == 3 and values[:2] == ["sh", "-c"]:
                if not values[2].startswith("set -f; for tool in "): return False
            elif values[:1] == ["ssh"] and values[-1:] == ["-V"] and self.side == "a":
                pass
            elif len(values) == 6 and values[0] == "sh":
                script = Path(values[1]).resolve()
                directory = Path(values[3]).resolve()
                if directory.parent != root / self.side or not directory.name.startswith(".ssh-workspace-task-") or script != directory / "supervisor.sh": return False
                if values[2] not in ("guard", "run", "seal"): return False
            else: return False
        except (ValueError, UnicodeError): return False
        def execute():
            environment = dict(os.environ, PATH=str(wrappers) + os.pathsep + os.environ.get("PATH", ""), TRANSFER_FIXTURE_ROOT=str(root))
            child = subprocess.Popen(values, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment, start_new_session=True)
            with children_lock: children.add(child)
            log(event="runner", pid=child.pid, directory=str(directory), side=self.side)
            def output(stream, error=False):
                try:
                    while True:
                        data = os.read(stream.fileno(), 8192)
                        if not data: break
                        (channel.sendall_stderr if error else channel.sendall)(data)
                except (EOFError, OSError): pass
            def input_():
                try:
                    while True:
                        data = channel.recv(8192)
                        if not data: break
                        child.stdin.write(data); child.stdin.flush()
                except (EOFError, OSError): pass
                finally:
                    try: child.stdin.close()
                    except OSError: pass
            for function, arguments in [(output, (child.stdout,)), (output, (child.stderr, True)), (input_, ())]:
                threading.Thread(target=function, args=arguments, daemon=True).start()
            code = child.wait()
            with children_lock: children.discard(child)
            log(event="runner-exit", pid=child.pid, code=code)
            try: channel.send_exit_status(code); channel.close()
            except (EOFError, OSError): pass
        threading.Thread(target=execute, daemon=True).start()
        return True

class SFTPSubsystem(paramiko.SFTPServer):
    def _send_server_version(self):
        kind, data = self._read_packet()
        if kind != paramiko.sftp.CMD_INIT: raise paramiko.SFTPError("Invalid SFTP version")
        message = paramiko.Message()
        message.add_int(3)
        message.add("posix-rename@openssh.com", "1")
        self._send_packet(paramiko.sftp.CMD_VERSION, message)
        return struct.unpack(">I", data[:4])[0]
    def finish_subsystem(self):
        try: self.sock.send_exit_status(0)
        except (EOFError, OSError): pass
        super().finish_subsystem()

def serve(client, side, key):
    transport = paramiko.Transport(client); transport.add_server_key(key)
    transport.set_subsystem_handler("sftp", SFTPSubsystem, Files, side=side)
    try:
        server = Server(side); server.transport = transport
        transport.start_server(server=server)
        while transport.is_active(): time.sleep(0.1)
    except (OSError, EOFError, paramiko.SSHException): pass
    finally: transport.close()

def listen(side, port):
    (root / side).mkdir(exist_ok=True)
    key = paramiko.RSAKey.generate(2048)
    key.write_private_key_file(str(root / (side + "-host-key")))
    listener = socket.socket(); listener.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); listener.bind(("127.0.0.1",port)); listener.listen(30)
    (root / (side + "-ready")).touch()
    while True:
        client, _ = listener.accept(); threading.Thread(target=serve,args=(client,side,key),daemon=True).start()

def shutdown(*_):
    with children_lock: pending = list(children)
    for child in pending:
        if child.poll() is None:
            try: os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError: pass
    for child in pending: child.wait()
    sys.exit(0)

signal.signal(signal.SIGTERM, shutdown)
signal.signal(signal.SIGINT, shutdown)
for side, port in [("a", args.port_a), ("b", args.port_b)]: threading.Thread(target=listen,args=(side,port),daemon=True).start()
try:
    while True: time.sleep(0.2)
except KeyboardInterrupt:
    shutdown()
