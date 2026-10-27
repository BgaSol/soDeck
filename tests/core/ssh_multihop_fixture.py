"""Isolated SSH protocol chain with per-hop fault injection, not OpenSSH.

Only each predecessor maps its successor's private hostname. Loopback listeners
are a test transport, not evidence of network namespace isolation.
"""
import atexit
import io
import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
import paramiko
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, NoEncryption

root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=True)
count = int(sys.argv[2])
target = subprocess.Popen([sys.executable, '-u', str(Path(__file__).with_name('ssh_fixture.py')), str(root / 'target')], stdout=subprocess.PIPE, text=True)
atexit.register(target.terminate)
target_port = json.loads(target.stdout.readline())['port']
client_key = paramiko.RSAKey.from_private_key_file(str(root / 'target/client-key'))
lock = threading.Lock()

def event(name, index, **fields):
    with lock:
        with (root / 'events.jsonl').open('a', encoding='utf-8') as out:
            out.write(json.dumps({'event': name, 'index': index, **fields}) + '\n')

def host_key():
    pem = Ed25519PrivateKey.generate().private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, NoEncryption()).decode()
    return paramiko.Ed25519Key.from_private_key(io.StringIO(pem))

listeners = []
keys = [(host_key(), host_key()) for _ in range(count)]
for _ in range(count):
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen(128)
    listeners.append(listener)

def marker(index, name):
    return (root / f'{index}-{name}').exists()

class Gateway(paramiko.ServerInterface):
    def __init__(self, index):
        self.index = index
    def get_allowed_auths(self, username):
        return 'password,publickey'
    def check_auth_password(self, username, password):
        ok = username == 'jump-user' and password == 'jump-test-password'
        event('auth', self.index, accepted=ok)
        return paramiko.AUTH_SUCCESSFUL if ok else paramiko.AUTH_FAILED
    def check_auth_publickey(self, username, supplied):
        ok = username == 'jump-user' and supplied == client_key
        event('key-auth', self.index, accepted=ok)
        return paramiko.AUTH_SUCCESSFUL if ok else paramiko.AUTH_FAILED
    def check_channel_request(self, kind, chanid):
        event('session-denied', self.index, kind=kind)
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    def check_channel_direct_tcpip_request(self, chanid, origin, destination):
        event('forward', self.index, host=destination[0], port=destination[1])
        deadline = time.monotonic() + 10
        while marker(self.index, 'pause-forward') and time.monotonic() < deadline:
            time.sleep(.01)
        expected = 'target.internal' if self.index + 1 == count else f'hop-{self.index + 1}.internal'
        return paramiko.OPEN_SUCCEEDED if destination == (expected, 22) and not marker(self.index, 'deny-forward') else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

def bridge(channel, index):
    next_port = target_port if index + 1 == count else listeners[index + 1].getsockname()[1]
    peer = socket.create_connection(('127.0.0.1', next_port))
    def copy(source, destination):
        try:
            while data := source.recv(32768):
                destination.sendall(data)
        except (OSError, EOFError):
            pass
        finally:
            try: destination.shutdown(socket.SHUT_WR)
            except (OSError, EOFError): pass
    thread = threading.Thread(target=copy, args=(channel, peer), daemon=True)
    thread.start()
    copy(peer, channel)
    peer.close()
    channel.close()
    thread.join(timeout=2)
    event('forward-close', index)

def connection(stream, index):
    transport = paramiko.Transport(stream)
    event('connection-open', index)
    try:
        while marker(index, 'pause-handshake'):
            time.sleep(.01)
        transport.add_server_key(keys[index][int(marker(index, 'rotate-host-key'))])
        transport.start_server(server=Gateway(index))
        while transport.is_active() and not marker(index, 'drop'):
            channel = transport.accept(.05)
            if channel is not None:
                threading.Thread(target=bridge, args=(channel, index), daemon=True).start()
    except (OSError, EOFError, paramiko.SSHException):
        pass
    finally:
        transport.close()
        event('connection-close', index)

def serve(index):
    while True:
        stream, _ = listeners[index].accept()
        threading.Thread(target=connection, args=(stream, index), daemon=True).start()

for index in range(count):
    threading.Thread(target=serve, args=(index,), daemon=True).start()
print(json.dumps({'port': listeners[0].getsockname()[1], 'targetPid': target.pid, 'count': count}), flush=True)
sys.stdin.read()
target.terminate()
target.wait(timeout=5)
os._exit(0)
