"""Isolated forwarding-only SSH fixture. No shell, filesystem or host commands."""
import os
import socket
import threading
import sys
from pathlib import Path
import paramiko

root = Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)
key_path = root / "host-key"
if not key_path.exists():
    paramiko.RSAKey.generate(2048).write_private_key_file(str(key_path))
key = paramiko.RSAKey.from_private_key_file(str(key_path))

def half_close_service(peer):
    with peer:
        chunks = []
        while True:
            data = peer.recv(32768)
            if not data:
                break
            chunks.append(data)
        # Reply only after EOF: detects forwarding implementations that close
        # both directions when a client half-closes its request.
        for chunk in chunks:
            peer.sendall(chunk)
        peer.shutdown(socket.SHUT_WR)

target = socket.socket()
target.bind(("127.0.0.1", 0))
target.listen(64)
def accept_targets():
    while True:
        peer, _ = target.accept()
        threading.Thread(target=half_close_service, args=(peer,), daemon=True).start()
threading.Thread(target=accept_targets, daemon=True).start()

class Server(paramiko.ServerInterface):
    def __init__(self):
        self.peers = {}
    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username, password) == ("forward", "test-only") else paramiko.AUTH_FAILED
    def get_allowed_auths(self, username):
        return "password"
    def check_channel_direct_tcpip_request(self, channel_id, origin, destination):
        if destination != ("echo.internal.invalid", 7):
            return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
        self.peers[channel_id] = socket.create_connection(target.getsockname())
        return paramiko.OPEN_SUCCEEDED

def copy(source, destination):
    try:
        while True:
            data = source.recv(32768)
            if not data:
                break
            destination.sendall(data)
        if isinstance(destination, paramiko.Channel):
            destination.shutdown_write()
        else:
            destination.shutdown(socket.SHUT_WR)
    except (OSError, EOFError):
        pass

def relay(channel, peer):
    writer = threading.Thread(target=copy, args=(channel, peer))
    writer.start()
    copy(peer, channel)
    writer.join(timeout=5)
    channel.close()
    peer.close()

def serve(client):
    transport = paramiko.Transport(client)
    # The isolated server has no system moduli file for DH group exchange.
    options = transport.get_security_options()
    options.kex = tuple(algorithm for algorithm in options.kex if "group-exchange" not in algorithm)
    transport.add_server_key(key)
    server = Server()
    try:
        transport.start_server(server=server)
        while transport.is_active():
            channel = transport.accept(0.1)
            if channel is not None:
                peer = server.peers.pop(channel.get_id(), None)
                if peer is not None:
                    threading.Thread(target=relay, args=(channel, peer), daemon=True).start()
                else:
                    channel.close()
    except (OSError, EOFError, paramiko.SSHException):
        pass
    finally:
        transport.close()
        for peer in server.peers.values():
            peer.close()

listener = socket.socket()
if os.name != "nt":
    # A killed fixture can leave accepted TCP tuples in TIME_WAIT on Unix.
    # Permit rebinding its same isolated port for reconnect/key-rotation tests.
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", int(sys.argv[2])))
listener.listen(64)
print(listener.getsockname()[1], flush=True)
while True:
    client, _ = listener.accept()
    threading.Thread(target=serve, args=(client,), daemon=True).start()
