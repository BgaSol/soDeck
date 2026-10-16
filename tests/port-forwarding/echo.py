"""Test-only TCP service inside the isolated OpenSSH container."""
import socket
import threading

def serve(peer):
    with peer:
        chunks = []
        while True:
            data = peer.recv(32768)
            if not data:
                break
            chunks.append(data)
        for data in chunks:
            peer.sendall(data)
        peer.shutdown(socket.SHUT_WR)

listener = socket.socket()
listener.bind(("127.0.0.1", 7))
listener.listen(64)
while True:
    peer, _ = listener.accept()
    threading.Thread(target=serve, args=(peer,), daemon=True).start()
