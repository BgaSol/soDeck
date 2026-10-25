"""Loopback SSH protocol fixture. PTY payloads are echoed, not executed as shell.

Usage: python -u tests/core/ssh_fixture.py <temporary-directory>
The first stdout line reports the assigned loopback port and test key paths.
"""
import base64
import os
import json
import socket
import sys
import threading
import time
from pathlib import Path

import paramiko
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, NoEncryption, BestAvailableEncryption
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=True)
host_key = paramiko.RSAKey.generate(2048)
rotated_host_key = paramiko.RSAKey.generate(2048)
client_key = paramiko.RSAKey.generate(2048)
client_key.write_private_key_file(str(root / "client-key"))
client_key.write_private_key_file(str(root / "client-key-encrypted"), password="test-passphrase")
for destination, encryption in [("client-openssh", NoEncryption()), ("client-openssh-encrypted", BestAvailableEncryption(b"test-passphrase"))]:
    (root / destination).write_bytes(client_key.key.private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, encryption))
ed25519 = Ed25519PrivateKey.generate()
for destination, encryption in [("client-ed25519", NoEncryption()), ("client-ed25519-encrypted", BestAvailableEncryption(b"test-passphrase"))]:
    (root / destination).write_bytes(ed25519.private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, encryption))
ed25519_key = paramiko.Ed25519Key.from_private_key_file(str(root / "client-ed25519"))
lock = threading.Lock()


def event(name, **fields):
    with lock:
        with (root / "events.jsonl").open("a", encoding="utf-8") as stream:
            stream.write(json.dumps({"event": name, **fields}) + "\n")


class Server(paramiko.ServerInterface):
    def get_allowed_auths(self, username):
        return "password,publickey"

    def check_auth_password(self, username, password):
        accepted = username == "test-user" and password == "isolated-test-password"
        event("password-auth", accepted=accepted)
        return paramiko.AUTH_SUCCESSFUL if accepted else paramiko.AUTH_FAILED

    def check_auth_publickey(self, username, key):
        accepted = username == "test-user" and key in (client_key, ed25519_key)
        event("publickey-auth", accepted=accepted)
        return paramiko.AUTH_SUCCESSFUL if accepted else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_pty_request(self, channel, term, width, height, pixelwidth, pixelheight, modes):
        event("pty", channel=channel.get_id(), width=width, height=height)
        return True

    def check_channel_window_change_request(self, channel, width, height, pixelwidth, pixelheight):
        event("resize", channel=channel.get_id(), width=width, height=height)
        return True

    def check_channel_shell_request(self, channel):
        # An explicit test-only gate exposes late shell replies deterministically.
        # The fixture is isolated per run; no production code reads this marker.
        if (root / "pause-shell").exists():
            event("shell-paused", channel=channel.get_id())
            deadline = time.monotonic() + 10
            while (root / "pause-shell").exists() and time.monotonic() < deadline:
                time.sleep(0.01)
        threading.Thread(target=echo, args=(channel,), daemon=True).start()
        return True


def echo(channel):
    event("shell-open", channel=channel.get_id())
    send_lock = threading.Lock()
    stopped = threading.Event()

    def flood():
        deadline = time.monotonic()
        sequence = 0
        try:
            while not stopped.is_set() and not channel.closed:
                prefix = f"FLOOD:{sequence:08d}:".encode()
                block = prefix + b"x" * (8191 - len(prefix)) + b"\n"
                with send_lock:
                    channel.sendall(block)
                sequence += 1
                deadline += 1 / 32  # 8192 bytes * 32 = 256 KiB/s per PTY.
                stopped.wait(max(0, deadline - time.monotonic()))
        except (OSError, EOFError):
            pass

    try:
        channel.sendall(b"fixture-ready\r\n")
        command = b"__SODECK_TEST_STREAM__"
        candidate = b""
        matching = True
        stream_started = False
        while True:
            data = channel.recv(32768)
            if not data:
                break
            if os.environ.get("SODECK_TEST_CAPTURE_INPUT") == "1":
                event("input-bytes", channel=channel.get_id(), bytes=base64.b64encode(data).decode("ascii"))
            # Echo interactive input as soon as it arrives, including a single
            # character without Enter. The control-line recognizer is separate
            # and bounded; it must not turn this fixture into a line-buffered UI.
            with send_lock:
                channel.sendall(data.replace(b"\r", b"\n"))
            for byte in data:
                if byte in (10, 13):
                    if candidate == command and not stream_started:
                        stream_started = True
                        threading.Thread(target=flood, daemon=True).start()
                    candidate = b""
                    matching = True
                elif matching:
                    candidate += bytes((byte,))
                    if not command.startswith(candidate):
                        candidate = b""
                        matching = False
    except (OSError, EOFError):
        pass
    finally:
        stopped.set()
        channel.close()
        event("shell-close", channel=channel.get_id())


def connection(stream):
    transport = paramiko.Transport(stream)
    transport.get_security_options().kex = ("diffie-hellman-group14-sha256",)
    event("connection-open")
    try:
        transport.add_server_key(rotated_host_key if (root / "rotate-host-key").exists() else host_key)
        transport.start_server(server=Server())
        channels = []
        while transport.is_active():
            channel = transport.accept(0.1)
            if channel is not None:
                channels.append(channel)
            channels = [channel for channel in channels if not channel.closed]
    except (OSError, EOFError, paramiko.SSHException):
        pass
    finally:
        transport.close()
        event("connection-close")


listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen(16)
print(json.dumps({"port": listener.getsockname()[1]}), flush=True)
while True:
    stream, _ = listener.accept()
    threading.Thread(target=connection, args=(stream,), daemon=True).start()
