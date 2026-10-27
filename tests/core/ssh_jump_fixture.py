"""Isolated real SSH protocol gateway. DNS mapping is synthetic, not OpenSSH.
The target is the existing echo fixture. The gateway refuses all session channels.
Only target.internal:22 is forwarded, to the fixture's private ephemeral endpoint.
"""
import atexit
import json
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
import paramiko

root = Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=True)
target = subprocess.Popen([sys.executable, '-u', str(Path(__file__).with_name('ssh_fixture.py')), str(root / 'target')], stdout=subprocess.PIPE, text=True)
atexit.register(target.terminate)
target_port = json.loads(target.stdout.readline())['port']
key = paramiko.RSAKey.generate(2048)
rotated = paramiko.RSAKey.generate(2048)
client_key = paramiko.RSAKey.from_private_key_file(str(root / 'target' / 'client-key'))
lock = threading.Lock()

def event(name, **fields):
    with lock:
        with (root / 'gateway-events.jsonl').open('a', encoding='utf-8') as out:
            out.write(json.dumps({'event': name, **fields}) + '\n')

class Gateway(paramiko.ServerInterface):
    def get_allowed_auths(self, username):
        return 'password,publickey'
    def check_auth_password(self, username, password):
        ok = username == 'jump-user' and password == 'jump-test-password'
        event('auth', accepted=ok)
        return paramiko.AUTH_SUCCESSFUL if ok else paramiko.AUTH_FAILED
    def check_auth_publickey(self, username, supplied):
        ok = username == 'jump-user' and supplied == client_key
        event('key-auth', accepted=ok)
        return paramiko.AUTH_SUCCESSFUL if ok else paramiko.AUTH_FAILED
    def check_channel_request(self, kind, chanid):
        event('session-denied', kind=kind)
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    def check_channel_direct_tcpip_request(self, chanid, origin, destination):
        event('forward', host=destination[0], port=destination[1])
        deadline = time.monotonic() + 10
        while (root / 'pause-forward').exists() and time.monotonic() < deadline:
            time.sleep(.01)
        return paramiko.OPEN_SUCCEEDED if destination == ('target.internal', 22) and not (root / 'deny-forward').exists() else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

def bridge(channel):
    peer = socket.create_connection(('127.0.0.1', target_port))
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
    a = threading.Thread(target=copy, args=(channel, peer), daemon=True)
    a.start()
    copy(peer, channel)
    peer.close()
    channel.close()
    event('forward-close')

def connection(stream):
    transport = paramiko.Transport(stream)
    event('connection-open')
    try:
        while (root / 'pause-handshake').exists():
            time.sleep(.01)
        transport.add_server_key(rotated if (root / 'rotate-host-key').exists() else key)
        transport.start_server(server=Gateway())
        while transport.is_active():
            if (root / 'drop-gateway').exists():
                break
            channel = transport.accept(.1)
            if channel:
                threading.Thread(target=bridge, args=(channel,), daemon=True).start()
    except (OSError, EOFError, paramiko.SSHException):
        pass
    finally:
        transport.close()
        event('connection-close')

listener = socket.socket()
listener.bind(('127.0.0.1', 0))
listener.listen(16)
print(json.dumps({'port': listener.getsockname()[1], 'targetPid': target.pid}), flush=True)

# EOF on the parent's pipe shuts down the child target as well as this gateway.
def lifetime():
    sys.stdin.read()
    target.terminate()
    target.wait(timeout=5)
    import os
    os._exit(0)
threading.Thread(target=lifetime, daemon=True).start()
while True:
    stream, _ = listener.accept()
    threading.Thread(target=connection, args=(stream,), daemon=True).start()
