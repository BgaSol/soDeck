"""Isolated loopback SSH endpoints for transfer integration tests.

Both use real SSH/SFTP. Only A accepts exec, and only the bundled task runner
under its fixture root; that runner invokes the system OpenSSH scp/ssh against B.
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

def log(**event):
    with lock:
        with (root / "events.jsonl").open("a") as stream:
            stream.write(json.dumps(event) + "\n")

key_path = root / "identity"
identity = paramiko.RSAKey.generate(2048)
identity.write_private_key_file(str(key_path))
wrappers = root / "bin"
wrappers.mkdir(exist_ok=True)
# Only the fixture source process receives this PATH. Existing user SSH config
# and keys are not modified. -F applies before production-enforced -o options.
(wrappers / "ssh").write_text("#!/bin/sh\nif [ -f " + shlex.quote(str(root / "deny-direct")) + " ] && [ \"$1\" != '-V' ]; then echo 'fixture: A cannot reach B' >&2; exit 255; fi\nexec /usr/bin/ssh -F /dev/null -i " + shlex.quote(str(key_path)) + ' "$@"\n')
(wrappers / "ssh").chmod(0o700)

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
            os.replace(self.path(source, True), target); self.after_commit(target); return paramiko.SFTP_OK
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
            if self.side != "a" or len(values) != 4 or values[:2] != ["exec", "python3"]: return False
            script, directory = Path(values[2]).resolve(), Path(values[3]).resolve()
            if directory.parent != root / "a" or not directory.name.startswith(".ssh-workspace-task-") or script != directory / "runner.py": return False
        except (ValueError, UnicodeError): return False
        def execute():
            environment = dict(os.environ, PATH=str(wrappers) + os.pathsep + os.environ.get("PATH", ""))
            child = subprocess.Popen(["/usr/bin/python3", str(script), str(directory)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment)
            children.add(child)
            log(event="runner", pid=child.pid, directory=str(directory))
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
            for function, values in [(output, (child.stdout,)), (output, (child.stderr, True)), (input_, ())]:
                threading.Thread(target=function, args=values, daemon=True).start()
            code = child.wait(); children.discard(child)
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

for side, port in [("a", args.port_a), ("b", args.port_b)]: threading.Thread(target=listen,args=(side,port),daemon=True).start()
try:
    while True: time.sleep(0.2)
except KeyboardInterrupt:
    for child in children: child.terminate()
