"""Isolated SSH PTY input observer; received bytes are echoed, never executed."""
import base64
import json
import socket
import sys
import threading
from pathlib import Path
import paramiko

root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=True)
host_key = paramiko.RSAKey.generate(2048)
event_lock = threading.Lock()


def event(name, **fields):
    with event_lock:
        with (root / "events.jsonl").open("a", encoding="utf-8") as stream:
            stream.write(json.dumps({"event": name, **fields}) + "\n")


def echo(channel):
    channel_id = channel.get_id()
    event("shell-open", channel=channel_id)
    try:
        channel.sendall(f"input-fixture-ready:{channel_id}\r\n".encode())
        while True:
            data = channel.recv(32768)
            if not data:
                break
            event("received", channel=channel_id, bytes=base64.b64encode(data).decode())
            channel.sendall(data.replace(b"\r", b"\n"))
    except (OSError, EOFError):
        pass
    finally:
        channel.close()
        event("shell-close", channel=channel_id)


class Server(paramiko.ServerInterface):
    def get_allowed_auths(self, username):
        return "password"

    def check_auth_password(self, username, password):
        accepted = username == "test-user" and password == "isolated-input-password"
        event("password-auth", accepted=accepted)
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
        threading.Thread(target=echo, args=(channel,), daemon=True).start()
        return True


def connection(stream):
    transport = paramiko.Transport(stream)
    transport.get_security_options().kex = ("diffie-hellman-group14-sha256",)
    event("connection-open")
    try:
        transport.add_server_key(host_key)
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
listener.listen()
print(json.dumps({"port": listener.getsockname()[1]}), flush=True)
while True:
    stream, _ = listener.accept()
    threading.Thread(target=connection, args=(stream,), daemon=True).start()
