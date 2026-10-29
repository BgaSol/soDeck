"""Isolated protocol fixture; predecessor-only names, not network namespaces.

Every middle SSH denies session channels. SOCKS replies are fragmented and the
target banner may immediately follow CONNECT. No application data is accessed.
"""
import io
import json
import logging
import os
import socket
import struct
import sys
import threading
import time
from pathlib import Path

import paramiko
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, NoEncryption

logging.getLogger('paramiko').setLevel(logging.CRITICAL)
root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=True)
kinds = sys.argv[2].split(',')
assert kinds[-1] == 'ssh'
lock = threading.Lock()
listeners = []
for _ in kinds:
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen(128)
    listeners.append(listener)


def event(name, index, **fields):
    with lock:
        with (root / 'events.jsonl').open('a', encoding='utf8') as out:
            out.write(json.dumps(dict(event=name, index=index, **fields)) + '\n')


def endpoint(index):
    # The loopback proxy is interpreted by its immediate SSH predecessor.
    return ('127.0.0.1', 12021) if kinds[index] == 'socks' and index and kinds[index - 1] == 'ssh' else (f'step-{index}.invalid', 22)


def copy(source, destination):
    try:
        while data := source.recv(32768):
            destination.sendall(data)
    except (OSError, EOFError):
        pass
    finally:
        try:
            destination.shutdown(socket.SHUT_WR)
        except (OSError, EOFError):
            pass


def relay(stream, index):
    peer = socket.create_connection(('127.0.0.1', listeners[index + 1].getsockname()[1]))
    worker = threading.Thread(target=copy, args=(stream, peer), daemon=True)
    worker.start()
    copy(peer, stream)
    peer.close()
    stream.close()
    worker.join(2)
    event('forward-close', index)


def key():
    pem = Ed25519PrivateKey.generate().private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, NoEncryption()).decode()
    return paramiko.Ed25519Key.from_private_key(io.StringIO(pem))


keys = [key() for _ in kinds]


class Server(paramiko.ServerInterface):
    def __init__(self, index):
        self.index = index

    def get_allowed_auths(self, username):
        return 'password'

    def check_auth_password(self, username, password):
        ok = username == 'fixture' and password == 'mixed-route-fixture'
        event('auth', self.index, accepted=ok)
        return paramiko.AUTH_SUCCESSFUL if ok else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, chanid):
        event('session', self.index, kind=kind)
        return paramiko.OPEN_SUCCEEDED if self.index == len(kinds) - 1 and kind == 'session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_direct_tcpip_request(self, chanid, origin, destination):
        event('forward', self.index, destination=destination)
        return paramiko.OPEN_SUCCEEDED if destination == endpoint(self.index + 1) else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_exec_request(self, channel, command):
        if (root / 'native-shell').exists() and command != b'fixture-proof':
            return self.check_channel_shell_request(channel)
        if command != b'fixture-proof':
            return False
        def send():
            time.sleep(.02)
            channel.sendall(b'MIXED-ROUTE-TARGET\n')
            channel.send_exit_status(0)
            channel.shutdown_write()
        threading.Thread(target=send, daemon=True).start()
        return True

    def check_channel_pty_request(self, channel, term, width, height, pixelwidth, pixelheight, modes):
        return self.index == len(kinds) - 1

    def check_channel_shell_request(self, channel):
        if not (root / 'native-shell').exists():
            return False
        def echo():
            try:
                time.sleep(.05)
                channel.sendall(b'FIXTURE_TARGET_READY\r\n')
                while data := channel.recv(4096):
                    channel.sendall(data)
            except (OSError, EOFError):
                pass
        threading.Thread(target=echo, daemon=True).start()
        return True


def ssh(stream, index):
    transport = paramiko.Transport(stream)
    try:
        transport.add_server_key(keys[index])
        transport.start_server(server=Server(index))
        while transport.is_active():
            channel = transport.accept(.05)
            if channel is not None and index + 1 < len(kinds):
                threading.Thread(target=relay, args=(channel, index), daemon=True).start()
    except (OSError, EOFError, paramiko.SSHException):
        pass
    finally:
        transport.close()


def exact(stream, size):
    result = b''
    while len(result) < size:
        chunk = stream.recv(size - len(result))
        if not chunk:
            raise EOFError()
        result += chunk
    return result


def socks(stream, index):
    try:
        while (root / f'pause-{index}').exists():
            time.sleep(.01)
        assert exact(stream, 3) == b'\x05\x01\x02'
        fault = root / f'method-eof-{index}'
        if fault.exists():
            if fault.read_text().strip() == '1':
                stream.sendall(b'\x05')
            event('method-eof', index)
            return
        stream.sendall(b'\x05\x02')
        version, size = exact(stream, 2)
        username = exact(stream, size)
        password = exact(stream, exact(stream, 1)[0])
        assert version == 1 and username == b'proxy' and password == b'mixed-proxy-fixture'
        event('proxy-auth', index)
        stream.sendall(b'\x01\x00')
        version, command, reserved, atyp = exact(stream, 4)
        assert (version, command, reserved) == (5, 1, 0)
        if atyp == 1:
            host = socket.inet_ntop(socket.AF_INET, exact(stream, 4))
        elif atyp == 4:
            host = socket.inet_ntop(socket.AF_INET6, exact(stream, 16))
        else:
            assert atyp == 3
            host = exact(stream, exact(stream, 1)[0]).decode()
        port = struct.unpack('!H', exact(stream, 2))[0]
        event('connect', index, host=host, port=port)
        if (root / f'local-candidates-{index}').exists():
            assert host in ('127.0.0.1', '::1') and port == 22
            rejected = root / f'candidate-rejected-{index}'
            if not rejected.exists():
                rejected.write_text(host)
                event('candidate-rejected', index, host=host)
                stream.sendall(b'\x05\x05\x00\x01\x00\x00\x00\x00\x00\x00')
                return
            assert rejected.read_text() != host
        else:
            assert (host, port) == endpoint(index + 1)
        for byte in b'\x05\x00\x00\x01\x00\x00\x00\x00\x00\x00':
            stream.sendall(bytes([byte]))
        relay(stream, index)
    except (OSError, EOFError, AssertionError):
        stream.close()


def connection(stream, index):
    event('open', index)
    try:
        (ssh if kinds[index] == 'ssh' else socks)(stream, index)
    finally:
        stream.close()
        event('close', index)


def serve(index):
    while True:
        stream, _ = listeners[index].accept()
        threading.Thread(target=connection, args=(stream, index), daemon=True).start()


for index in range(len(kinds)):
    threading.Thread(target=serve, args=(index,), daemon=True).start()
print(json.dumps({'steps': [dict(kind=kind, host='127.0.0.1' if index == 0 else endpoint(index)[0], port=listeners[0].getsockname()[1] if index == 0 else endpoint(index)[1]) for index, kind in enumerate(kinds)]}), flush=True)
sys.stdin.read()
os._exit(0)
