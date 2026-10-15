"""Loopback SSH jump server: forwards only the explicitly configured fixture target.

SSH_WORKSPACE_FIXTURE_PORT chooses A's listener; SSH_WORKSPACE_JUMP_TARGET_PORT
chooses B. The logical destination must be target.internal.invalid:22. Each
instance needs its own SSH_WORKSPACE_FIXTURE_DIR so A and B have distinct keys.
"""
import os
import select
import socket
import threading
import time

import paramiko
import ssh_fixture as fixture


class JumpServer(fixture.Server):
    def __init__(self):
        self.destinations = {}

    def check_channel_direct_tcpip_request(self, chanid, origin, destination):
        fixture.log("forward-request", host=destination[0], port=destination[1])
        if (fixture.QA / "deny-forward").exists():
            return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
        if destination != ("target.internal.invalid", 22):
            return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
        try:
            peer = socket.create_connection(("127.0.0.1", int(os.environ["SSH_WORKSPACE_JUMP_TARGET_PORT"])), timeout=2)
            self.destinations[chanid] = peer
            return paramiko.OPEN_SUCCEEDED
        except OSError:
            return paramiko.OPEN_FAILED_CONNECT_FAILED


def relay(channel, peer):
    sent = received = 0
    try:
        while not channel.closed:
            ready, _, _ = select.select([channel, peer], [], [], 0.1)
            for source in ready:
                data = source.recv(32768)
                if not data:
                    return
                if source is channel:
                    peer.sendall(data)
                    sent += len(data)
                else:
                    channel.sendall(data)
                    received += len(data)
    except (OSError, EOFError):
        pass
    finally:
        peer.close()
        try:
            channel.close()
        except (OSError, EOFError):
            pass
        fixture.log("forward-close", sent=sent, received=received)


def serve(client):
    transport = paramiko.Transport(client)
    # Match the target fixture: neither lightweight server has a DH moduli file.
    options = transport.get_security_options()
    options.kex = tuple(algorithm for algorithm in options.kex if "group-exchange" not in algorithm)
    transport.add_server_key(fixture.HOST_KEY)
    server = JumpServer()
    fixture.log("jump-open")
    try:
        transport.start_server(server=server)
        while transport.is_active():
            if (fixture.QA / "disconnect").exists():
                break
            channel = transport.accept(0.1)
            if channel is not None:
                peer = server.destinations.pop(channel.get_id(), None)
                if peer is not None:
                    threading.Thread(target=relay, args=(channel, peer), daemon=True).start()
                else:
                    channel.close()
    except (paramiko.SSHException, OSError, EOFError):
        pass
    finally:
        transport.close()
        for peer in server.destinations.values():
            peer.close()
        fixture.log("jump-close")


if __name__ == "__main__":
    listener = socket.socket()
    listener.bind(("127.0.0.1", int(os.environ["SSH_WORKSPACE_FIXTURE_PORT"])))
    listener.listen(32)
    print(f"Jump fixture ready on {listener.getsockname()}", flush=True)
    try:
        while True:
            client, _ = listener.accept()
            threading.Thread(target=serve, args=(client,), daemon=True).start()
    except KeyboardInterrupt:
        listener.close()
