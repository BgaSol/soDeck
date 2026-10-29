"""SOCKS fixture for the dedicated T-027 VM; bind only inside its namespace."""
import json
import select
import socket
import struct
import sys
import threading

port = int(sys.argv[1])
log = sys.argv[2]
lock = threading.Lock()


def exact(stream, count):
    result = b''
    while len(result) < count:
        data = stream.recv(count - len(result))
        if not data:
            raise EOFError()
        result += data
    return result


def serve(stream):
    peer = None
    try:
        assert exact(stream, 3) == b'\x05\x01\x00'
        stream.sendall(b'\x05\x00')
        version, command, reserved, atyp = exact(stream, 4)
        assert (version, command, reserved) == (5, 1, 0)
        if atyp == 1:
            host = socket.inet_ntop(socket.AF_INET, exact(stream, 4))
        elif atyp == 4:
            host = socket.inet_ntop(socket.AF_INET6, exact(stream, 16))
        else:
            assert atyp == 3
            host = exact(stream, exact(stream, 1)[0]).decode()
        target_port = struct.unpack('!H', exact(stream, 2))[0]
        with lock:
            with open(log, 'a') as out:
                out.write(json.dumps(dict(host=host, port=target_port, proxyPort=port)) + '\n')
        peer = socket.create_connection((host, target_port), timeout=5)
        peer.settimeout(None)
        stream.sendall(b'\x05\x00\x00\x01\x00\x00\x00\x00\x00\x00')
        while True:
            ready, _, _ = select.select([stream, peer], [], [], 15)
            if not ready:
                continue
            for source in ready:
                data = source.recv(32768)
                if not data:
                    return
                (peer if source is stream else stream).sendall(data)
    except (OSError, EOFError, AssertionError):
        pass
    finally:
        stream.close()
        if peer:
            peer.close()


listener = socket.socket()
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(('0.0.0.0' if port == 2223 else '127.0.0.1', port))
listener.listen(128)
while True:
    stream, _ = listener.accept()
    threading.Thread(target=serve, args=(stream,), daemon=True).start()
