"""Loopback-only SSH/SFTP fixture. Docker responses are simulated; no host commands run.

Install: python -m pip install -r tests/requirements.txt
Run: python tests/ssh_fixture.py
Test login: ops / fixture-password at 127.0.0.1:22222.

Extra samples: /editing contains UTF-8, binary, invalid UTF-8, and >1 MiB files;
/download contains nested/empty/hidden/Unicode entries, two synthetic links,
and an 8 MiB slow.bin. Write a delay in seconds to .qa/sftp-read-delay to slow
each slow.bin read. Create .qa/sftp-read-fail to fail reads; remove to retry.
"""
import json
import logging
import errno
import itertools
import os
import posixpath
import shlex
import socket
import stat
import struct
import threading
import time
from pathlib import Path

import paramiko

QA = Path(__file__).resolve().parents[1] / ".qa"
QA.mkdir(exist_ok=True)
ROOT = QA / "remote"
(ROOT / "projects").mkdir(parents=True, exist_ok=True)
(ROOT / "readme.txt").write_text("SSH Workspace SFTP fixture\n", encoding="utf-8")
(ROOT / "projects" / "hello.txt").write_text("hello\n", encoding="utf-8")
(ROOT / "editing").mkdir(exist_ok=True)
(ROOT / "editing" / "config.yaml").write_text("# SSH Workspace 编辑测试\nport: 8080\nenabled: true\n", encoding="utf-8")
(ROOT / "editing" / "binary.dat").write_bytes(b"\x00\x01\xff\x00fixture")
(ROOT / "editing" / "invalid-utf8.txt").write_bytes(b"\xff\xfelegacy text")
(ROOT / "editing" / "too-large.txt").write_bytes(b"x" * (1024 * 1024 + 1))
(ROOT / "download" / "nested" / "empty").mkdir(parents=True, exist_ok=True)
(ROOT / "download" / "readme.txt").write_text("download fixture\n", encoding="utf-8")
(ROOT / "download" / ".hidden").write_text("hidden fixture\n", encoding="utf-8")
(ROOT / "download" / "nested" / "中文.txt").write_text("递归下载测试\n", encoding="utf-8")
(ROOT / "download" / "slow.bin").write_bytes(bytes(range(256)) * 32768)
# Synthetic links work on Windows without Developer Mode or administrator rights.
LINKS = {"/download/loop-link": "/download", "/download/file-link": "/download/readme.txt"}
KEY = QA / "ssh-host-key"
if not KEY.exists():
    paramiko.RSAKey.generate(2048).write_private_key_file(str(KEY))
HOST_KEY = paramiko.RSAKey.from_private_key_file(str(KEY))
CID = "a" * 64
LOG_LOCK = threading.Lock()
CONNECTION_IDS = itertools.count(1)
PARAMIKO_LOG = logging.FileHandler(QA / "ssh-paramiko.log", encoding="utf-8")
PARAMIKO_LOG.setFormatter(logging.Formatter("%(asctime)s %(threadName)s %(levelname)s %(message)s"))
PARAMIKO_LOGGER = logging.getLogger("paramiko")
PARAMIKO_LOGGER.addHandler(PARAMIKO_LOG)
PARAMIKO_LOGGER.setLevel(logging.DEBUG)


def log(event, **fields):
    with LOG_LOCK:
        with (QA / "ssh-events.jsonl").open("a", encoding="utf-8") as stream:
            stream.write(json.dumps({"event": event, "time": time.time(), **fields}) + "\n")


class FileHandle(paramiko.SFTPHandle):
    def __init__(self, flags, stream, path):
        super().__init__(flags)
        self.readfile = stream
        self.writefile = stream
        self.path = path

    def read(self, offset, length):
        # Tests control these files; delays/failures affect downloads only.
        delay_file = QA / "sftp-read-delay"
        if self.path.endswith("/slow.bin") and delay_file.exists():
            try:
                time.sleep(min(2.0, max(0.0, float(delay_file.read_text().strip()))))
            except ValueError:
                time.sleep(0.1)
        if self.path.endswith("/slow.bin") and (QA / "sftp-read-fail").exists():
            log("sftp-read-error", path=self.path, offset=offset)
            return paramiko.SFTP_FAILURE
        result = super().read(offset, length)
        log("sftp-read", path=self.path, offset=offset, size=len(result) if isinstance(result, bytes) else 0)
        return result

    def write(self, offset, data):
        result = super().write(offset, data)
        log("sftp-write", path=self.path, offset=offset, size=len(data), status=result)
        return result

    def stat(self):
        try:
            return paramiko.SFTPAttributes.from_stat(os.fstat(self.readfile.fileno()))
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def chattr(self, attributes):
        try:
            if attributes.st_size is not None:
                self.writefile.truncate(attributes.st_size)
            return paramiko.SFTP_OK
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)


class Files(paramiko.SFTPServerInterface):
    def local(self, path, follow=True):
        normalized = self.canonicalize(path)
        if follow:
            normalized = LINKS.get(normalized, normalized)
        local = (ROOT / normalized.lstrip("/")).resolve()
        if not local.is_relative_to(ROOT.resolve()):
            raise PermissionError(errno.EACCES, "outside fixture")
        return local

    def canonicalize(self, path):
        return posixpath.normpath("/" + path.lstrip("/"))

    def list_folder(self, path):
        try:
            delay_file = QA / "sftp-delay"
            if delay_file.exists() and path == "/projects":
                time.sleep(2)
            output = []
            for local in self.local(path).iterdir():
                attributes = paramiko.SFTPAttributes.from_stat(local.lstat())
                attributes.filename = local.name
                output.append(attributes)
            for link in LINKS:
                if posixpath.dirname(link) == self.canonicalize(path):
                    attributes = self.lstat(link)
                    attributes.filename = posixpath.basename(link)
                    output.append(attributes)
            log("sftp-list", path=path)
            return output
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def stat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(self.local(path).stat())
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def lstat(self, path):
        try:
            path = self.canonicalize(path)
            if path in LINKS:
                attributes = paramiko.SFTPAttributes()
                attributes.st_mode = stat.S_IFLNK | 0o777
                attributes.st_size = len(LINKS[path])
                attributes.st_mtime = attributes.st_atime = int(time.time())
                attributes.st_uid = attributes.st_gid = 1000
                return attributes
            return paramiko.SFTPAttributes.from_stat(self.local(path, follow=False).lstat())
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def readlink(self, path):
        return LINKS.get(self.canonicalize(path), paramiko.SFTP_NO_SUCH_FILE)

    def open(self, path, flags, attributes):
        try:
            local = self.local(path)
            descriptor = os.open(local, flags | getattr(os, "O_BINARY", 0), attributes.st_mode or 0o666)
            mode = "r+b" if flags & os.O_RDWR else "wb" if flags & os.O_WRONLY else "rb"
            stream = os.fdopen(descriptor, mode)
            log("sftp-open", path=path, flags=flags)
            return FileHandle(flags, stream, self.canonicalize(path))
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def chattr(self, path, attributes):
        try:
            paramiko.SFTPServer.set_file_attr(str(self.local(path)), attributes)
            log("sftp-chattr", path=path)
            return paramiko.SFTP_OK
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def rename(self, oldpath, newpath):
        try:
            destination = self.local(newpath)
            if destination.exists():
                return paramiko.SFTP_FAILURE
            self.local(oldpath).rename(destination)
            log("sftp-rename", source=oldpath, destination=newpath)
            return paramiko.SFTP_OK
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def posix_rename(self, oldpath, newpath):
        try:
            os.replace(self.local(oldpath), self.local(newpath))
            log("sftp-posix-rename", source=oldpath, destination=newpath)
            return paramiko.SFTP_OK
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def remove(self, path):
        try:
            self.local(path).unlink()
            return paramiko.SFTP_OK
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)

    def mkdir(self, path, attributes):
        try:
            self.local(path).mkdir(mode=attributes.st_mode or 0o777)
            return paramiko.SFTP_OK
        except OSError as error:
            return paramiko.SFTPServer.convert_errno(error.errno)


class FixtureSFTPServer(paramiko.SFTPServer):
    def _send_server_version(self):
        # Paramiko implements posix-rename dispatch but does not advertise it.
        # Advertise the extension that Files.posix_rename actually implements,
        # so clients can negotiate atomic replacement just as with OpenSSH.
        kind, data = self._read_packet()
        if kind != paramiko.sftp.CMD_INIT:
            raise paramiko.SFTPError("Incompatible sftp protocol")
        version = struct.unpack(">I", data[:4])[0]
        message = paramiko.Message()
        message.add_int(3)
        message.add("check-file", "md5,sha1")
        if not (QA / "sftp-no-posix-rename").exists():
            message.add("posix-rename@openssh.com", "1")
        self._send_packet(paramiko.sftp.CMD_VERSION, message)
        return version


def terminal(channel, target):
    log("terminal-open", target=target, channel=channel.get_id())
    try:
        channel.sendall(f"\r\nFixture {target} ready\r\nops@{target}:~$ ".encode())
        pending = bytearray()
        while not channel.closed:
            data = channel.recv(4096)
            if not data:
                break
            for char in data:
                if char in (10, 13):
                    command = pending.decode("utf-8", "replace")
                    pending.clear()
                    log("terminal-input", target=target, command=command)
                    if command == "exit":
                        channel.send_exit_status(0)
                        return
                    if command.startswith("later "):
                        marker = command[6:]
                        def delayed_output(message=marker):
                            try:
                                channel.sendall(f"\r\n{message}\r\nops@{target}:~$ ".encode())
                            except (EOFError, OSError):
                                pass
                        threading.Timer(0.8, delayed_output).start()
                        continue
                    response = command[5:] if command.startswith("echo ") else f"{target}: {command}"
                    channel.sendall(f"\r\n{response}\r\nops@{target}:~$ ".encode())
                elif char in (8, 127):
                    if pending:
                        pending.pop()
                        channel.sendall(b"\b \b")
                else:
                    pending.append(char)
                    channel.sendall(bytes([char]))
    except (EOFError, OSError):
        pass
    finally:
        try:
            channel.close()
        except (EOFError, OSError):
            pass
        log("terminal-close", target=target)


def container_logs(channel, command):
    args = shlex.split(command)
    container = args[-1]
    tail = int(args[args.index("--tail") + 1])
    following = "--follow" in args
    mode_file = QA / "logs-mode"
    mode = mode_file.read_text().strip() if mode_file.exists() else "normal"
    log("logs-open", container=container, following=following, tail=tail)
    timestamp = "2026-10-14T09:00:00.000000000Z "
    try:
        if mode == "unsupported":
            channel.sendall_stderr(b"configured logging driver does not support reading\n")
            channel.send_exit_status(1)
            return
        if mode != "empty":
            channel.sendall((timestamp + "fixture history stdout\n").encode())
            channel.sendall_stderr((timestamp + "fixture history stderr\n").encode())
            data = (timestamp + "中文日志 \x1b[32mgreen\x1b[0m\n").encode()
            # Force a Unicode code point and an ANSI sequence across writes.
            cut = len(timestamp) + 1
            channel.sendall(data[:cut])
            time.sleep(0.08)
            channel.sendall(data[cut:])
        if mode == "burst":
            channel.sendall("".join(timestamp + f"burst-{n} " + "x" * 120 + "\n" for n in range(12000)).encode())
        if following:
            tick = 0
            while not channel.closed:
                time.sleep(0.15)
                if (QA / "logs-disconnect").exists():
                    channel.get_transport().close()
                    return
                if mode != "empty":
                    channel.sendall((timestamp + f"live-{tick}\n").encode())
                tick += 1
        else:
            channel.send_exit_status(0)
    except (EOFError, OSError):
        pass
    finally:
        try:
            channel.close()
        except (EOFError, OSError):
            pass
        log("logs-close", container=container)


def execute(channel, command):
    log("exec", command=command)
    mode_file = QA / "docker-mode"
    mode = mode_file.read_text().strip() if mode_file.exists() else "running"
    code, output, error = 0, "", ""
    if "command -v docker" in command:
        code = 1 if mode == "missing" else 0
    elif "context inspect" in command:
        output = "unix:///var/run/docker.sock\n"
    elif "docker info" in command:
        if mode in ("permission", "unreachable"):
            code, error = 1, "permission denied" if mode == "permission" else "Cannot connect to the Docker daemon"
        elif mode == "timeout":
            time.sleep(15)
        else:
            output = '"27.5.1"\n'
    elif " image ls " in command:
        output = json.dumps({"ID": "sha256:" + "d" * 64, "Repository": "nginx", "Tag": "alpine", "Size": "50MB"}) + "\n"
    elif " ps -a " in command:
        output = "\n".join(json.dumps({"ID": cid, "Names": name, "Image": "nginx:alpine", "State": state, "Status": state}) for cid, name, state in [
            (CID, "web-fixture", "running"), ("b" * 64, "stopped-fixture", "exited"), ("c" * 64, "paused-fixture", "paused")]) + "\n"
    elif " inspect --type container " in command:
        state = {"Running": mode != "stopped" and "b" * 64 not in command, "Paused": mode == "paused", "Restarting": False}
        metadata = {"State": state, "Config": {"Tty": mode == "tty"}} if "{{json .}}" in command else state
        output = json.dumps(metadata) + "\n"
    elif " logs --tail " in command:
        container_logs(channel, command)
        return
    elif " exec -it " in command:
        terminal(channel, "container")
        return
    elif " exec " in command:
        if mode == "no-shell" or "/bin/bash" in command:
            code, error = 127, "exec: no such file or directory"
    else:
        code, error = 127, "unsupported fixture command"
    try:
        if output:
            channel.sendall(output.encode())
        if error:
            channel.sendall_stderr(error.encode())
        channel.send_exit_status(code)
        channel.shutdown_write()
        channel.close()
    except (EOFError, OSError):
        pass


class Server(paramiko.ServerInterface):
    username = ""
    def check_auth_password(self, username, password):
        self.username = username
        if username == "ops-delay" and (QA / "auth-delay").exists():
            time.sleep(2)
            log("delayed-auth-finished", username=username)
        return paramiko.AUTH_SUCCESSFUL if username in ("ops", "ops-delay") and password == "fixture-password" else paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return "password"

    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_pty_request(self, channel, term, width, height, pixelwidth, pixelheight, modes):
        log("pty", cols=width, rows=height)
        return True

    def check_channel_window_change_request(self, channel, width, height, pixelwidth, pixelheight):
        log("resize", cols=width, rows=height)
        return True

    def check_channel_shell_request(self, channel):
        threading.Thread(target=terminal, args=(channel, "host"), daemon=True).start()
        return True

    def check_channel_exec_request(self, channel, command):
        # Wait until Paramiko has sent the success reply before closing short commands.
        threading.Timer(0.03, execute, args=(channel, command.decode())).start()
        return True


def accept(client):
    connection_id = next(CONNECTION_IDS)
    accepted_at = time.monotonic()
    log("connection-open", connection_id=connection_id, peer=client.getpeername())
    transport = paramiko.Transport(client)
    transport.banner_timeout = transport.handshake_timeout = 15
    transport.auth_timeout = 15
    transport.set_log_channel(f"paramiko.connection-{connection_id}")
    # This lightweight server has no system moduli file for DH group exchange.
    options = transport.get_security_options()
    options.kex = tuple(algorithm for algorithm in options.kex if "group-exchange" not in algorithm)
    transport.add_server_key(HOST_KEY)
    transport.set_subsystem_handler("sftp", FixtureSFTPServer, Files)
    channels = []
    server = Server()
    try:
        transport.start_server(server=server)
        log("handshake-complete", connection_id=connection_id, elapsed=time.monotonic() - accepted_at)
        while transport.is_active():
            channel = transport.accept(1)
            if channel:
                channels.append(channel)
            channels = [channel for channel in channels if not channel.closed]
    except (EOFError, OSError, paramiko.SSHException) as error:
        log("connection-error", connection_id=connection_id, username=server.username, error=str(error))
    finally:
        transport.close()
        log("connection-close", connection_id=connection_id, username=server.username)


if __name__ == "__main__":
    listener = socket.socket()
    listener.bind(("127.0.0.1", 22222))
    listener.listen(128)
    print("SSH/SFTP fixture ready on 127.0.0.1:22222", flush=True)
    try:
        while True:
            client, _ = listener.accept()
            threading.Thread(target=accept, args=(client,), daemon=True).start()
    except KeyboardInterrupt:
        listener.close()
