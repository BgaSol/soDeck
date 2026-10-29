"""Isolated SOCKS5 wire service + real SSH protocol target; DNS mapping is synthetic.
The proxy never accepts SSH. stdin EOF shuts down both services.
"""
import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=True)
target = None
external_port = int(os.environ.get('SODECK_TEST_SOCKS_TARGET_PORT', '0'))
if not external_port:
    target = subprocess.Popen([sys.executable, '-u', str(Path(__file__).with_name('ssh_fixture.py')), str(root / 'target')], stdout=subprocess.PIPE, text=True)
target_port = external_port or json.loads(target.stdout.readline())['port']
lock = threading.Lock()

def event(name, **fields):
    with lock:
        with (root / 'proxy-events.jsonl').open('a', encoding='utf-8') as out:
            out.write(json.dumps({'event': name, **fields}) + '\n')

def exact(peer, count):
    result = b''
    while len(result) < count:
        data = peer.recv(count - len(result))
        if not data:
            raise EOFError()
        result += data
    return result

def connection(peer, auth):
    upstream = None
    event('open')
    try:
        peer.settimeout(10)
        version, count = exact(peer, 2)
        methods = exact(peer, count)
        event('greeting', version=version)
        while (root / 'pause-negotiate').exists():
            time.sleep(.01)
        if version != 5:
            return
        method = 2 if auth else 0
        peer.sendall(bytes([5, method if method in methods else 255]))
        if method not in methods:
            return
        if auth:
            version, size = exact(peer, 2)
            username = exact(peer, size)
            password = exact(peer, exact(peer, 1)[0])
            accepted = version == 1 and username == b'proxy-user' and password == b'proxy-password'
            peer.sendall(bytes([1, 0 if accepted else 1]))
            event('proxy-auth', accepted=accepted)
            if not accepted:
                return
        version, command, reserved, atyp = exact(peer, 4)
        if atyp == 3:
            host = exact(peer, exact(peer, 1)[0]).decode('ascii')
        elif atyp in (1, 4):
            host = socket.inet_ntop(socket.AF_INET if atyp == 1 else socket.AF_INET6, exact(peer, 4 if atyp == 1 else 16))
        else:
            return
        port = int.from_bytes(exact(peer, 2), 'big')
        event('connect', host=host, port=port, command=command)
        while (root / 'pause-connect').exists():
            time.sleep(.01)
        allowed = version == 5 and command == 1 and reserved == 0 and host in ('target.internal', '127.0.0.1', '::1') and port in (22, target_port) and not (root / 'deny-connect').exists()
        denied_ip = root / 'deny-ip'
        if denied_ip.exists() and host == denied_ip.read_text():
            allowed = False
            event('denied-ip', host=host)
        reply = bytes([5, 0 if allowed else 4, 0, 1, 0, 0, 0, 0, 0, 0])
        if not allowed:
            peer.sendall(reply)
            return
        upstream = socket.create_connection(('127.0.0.1', target_port))
        banner = upstream.recv(4096)
        # Send split reply followed immediately by SSH bytes to test exact framing.
        peer.sendall(reply[:3])
        time.sleep(.005)
        peer.sendall(reply[3:] + banner)
        peer.settimeout(None)
        def copy(source, destination):
            try:
                while data := source.recv(32768):
                    destination.sendall(data)
            except (OSError, EOFError):
                pass
            finally:
                try:
                    destination.shutdown(socket.SHUT_WR)
                except OSError:
                    pass
        writer = threading.Thread(target=copy, args=(peer, upstream), daemon=True)
        writer.start()
        copy(upstream, peer)
        writer.join(timeout=1)
    except (OSError, EOFError):
        pass
    finally:
        peer.close()
        if upstream:
            upstream.close()
        event('close')

def serve(auth):
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen(32)
    def accept():
        while True:
            peer, _ = listener.accept()
            threading.Thread(target=connection, args=(peer, auth), daemon=True).start()
    threading.Thread(target=accept, daemon=True).start()
    return listener.getsockname()[1]

print(json.dumps({'port': serve(False), 'authPort': serve(True), 'targetPort': target_port}), flush=True)
sys.stdin.read()
if target:
    target.terminate()
    target.wait(timeout=5)
os._exit(0)
