"""Isolated Paramiko fault fixture, not evidence of a real Windows/Linux sshd.

Each SSH node recognizes only the next node; only the last recognizes targets.
A withheld channel-open reply leaves the transport thread free to serve existing
channels, reproducing the libssh2 pending-open hazard without blocking server I/O.
"""
import io
import json
import socket
import sys
import threading
import time
from pathlib import Path
import paramiko
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, NoEncryption

root = Path(sys.argv[1]); root.mkdir(parents=True, exist_ok=True)
count = int(sys.argv[2]) if len(sys.argv) > 2 else 3
lock = threading.Lock()
transports = []
transport_nodes = {}
blocked = set()

def event(name, **fields):
    with lock:
        with (root / 'events.jsonl').open('a', encoding='utf-8') as out:
            out.write(json.dumps({'event': name, 'time': time.monotonic(), **fields}) + '\n')

def host_key():
    key = Ed25519PrivateKey.generate().private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, NoEncryption()).decode()
    return paramiko.Ed25519Key.from_private_key(io.StringIO(key))

listeners = []
keys = [host_key() for _ in range(count)]
for _ in range(count):
    listener = socket.socket(); listener.bind(('127.0.0.1', 0)); listener.listen(128)
    listeners.append(listener)

class Server(paramiko.ServerInterface):
    def __init__(self, index): self.index, self.targets = index, {}
    def get_allowed_auths(self, username): return 'password'
    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username, password) == ('network-test', 'isolated-network-password') else paramiko.AUTH_FAILED
    def check_channel_request(self, kind, channel):
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    def check_channel_direct_tcpip_request(self, channel, origin, destination):
        event('target', node=self.index, host=destination[0], port=destination[1])
        allowed = (destination == (f'node-{self.index+1}.internal', 22)) if self.index < count-1 else destination[0] in ['echo.internal', 'other.internal', 'http.internal', 'second.internal']
        if allowed:
            self.targets[channel] = destination
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_CONNECT_FAILED

class Transport(paramiko.Transport):
    def _parse_channel_open(self, message):
        copy = paramiko.Message(message.asbytes())
        kind = copy.get_text()
        copy.get_int(); copy.get_int(); copy.get_int()
        if kind == 'direct-tcpip' and copy.get_text() == 'stuck.internal':
            event('withheld-open', node=count-1)
            return
        super()._parse_channel_open(message)

def bridge(channel, index):
    peer = socket.create_connection(listeners[index+1].getsockname())
    def copy(source, destination):
        try:
            while data := source.recv(65536): destination.sendall(data)
        except (OSError, EOFError): pass
        finally:
            try: destination.shutdown(socket.SHUT_WR)
            except (OSError, EOFError): pass
    t = threading.Thread(target=copy, args=(channel, peer), daemon=True); t.start()
    copy(peer, channel); t.join(timeout=2); peer.close(); channel.close()

def echo(channel):
    try:
        while data := channel.recv(65536): channel.sendall(data)
        channel.sendall(b'AFTER-EOF')
        channel.shutdown_write()
    except (OSError, EOFError): pass
    finally: channel.close()

def http(channel, host):
    try:
        data = b''
        while b'\r\n\r\n' not in data and len(data) <= 65536:
            part = channel.recv(8192)
            if not part: return
            data += part
        marker = ('REMOTE-' + host).encode()
        channel.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: ' + str(len(marker)).encode() + b'\r\nConnection: close\r\n\r\n' + marker)
        channel.shutdown_write()
        while channel.recv(8192): pass
    except (OSError, EOFError): pass
    finally: channel.close()

def connection(socket, index):
    transport = Transport(socket); server = Server(index)
    with lock:
        transports.append(transport); transport_nodes[transport] = index
    event('connection-open', node=index)
    try:
        transport.add_server_key(keys[index]); transport.start_server(server=server)
        while transport.is_active():
            channel = transport.accept(.05)
            if channel is not None:
                host = server.targets[channel.get_id()][0]
                handler = bridge if index < count-1 else http if host in ['http.internal', 'second.internal'] else echo
                args = (channel, index) if handler == bridge else (channel, host) if handler == http else (channel,)
                threading.Thread(target=handler, args=args, daemon=True).start()
    except (OSError, EOFError, paramiko.SSHException): pass
    finally:
        transport.close(); event('connection-close', node=index)

def serve(index):
    while True:
        try: peer, _ = listeners[index].accept()
        except OSError: return
        if index in blocked:
            event('blocked-connection', node=index); peer.close(); continue
        threading.Thread(target=connection, args=(peer, index), daemon=True).start()

for index in range(count): threading.Thread(target=serve, args=(index,), daemon=True).start()
print(json.dumps({'port':listeners[0].getsockname()[1], 'nodes':count}), flush=True)
for line in sys.stdin:
    command=json.loads(line)
    if command['action']=='block':
        blocked.add(command['node'])
        for transport in list(transports):
            if transport_nodes[transport]==command['node']: transport.close()
    elif command['action']=='unblock': blocked.discard(command['node'])
    event('control', **command)
for transport in transports: transport.close()
for listener in listeners: listener.close()
