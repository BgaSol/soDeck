"""Only deployed inside the dedicated T-014 QEMU overlay."""
import http.server
import socket
import ssl
import sys
import threading
import time

marker = sys.argv[1]
class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *_): pass
    def do_GET(self):
        if self.path == '/continuous':
            self.send_response(200); self.send_header('Connection', 'close'); self.end_headers()
            try:
                while True:
                    self.wfile.write(b'x'*1024); self.wfile.flush(); time.sleep(.01)
            except (BrokenPipeError, ConnectionResetError): pass
            self.close_connection = True; return
        if self.path == '/events':
            self.send_response(200); self.send_header('Content-Type', 'text/event-stream'); self.send_header('Connection', 'close'); self.end_headers()
            for index in range(3):
                self.wfile.write(f'data: {index}\n\n'.encode()); self.wfile.flush(); time.sleep(.05)
            self.close_connection = True; return
        if self.path == '/upgrade':
            self.send_response(101); self.send_header('Upgrade', 'websocket'); self.send_header('Connection', 'Upgrade'); self.end_headers(); self.wfile.write(b'UPGRADE-TAIL'); self.wfile.flush()
            self.close_connection = True; return
        count = 64*1024*1024 if self.path == '/large' else 2*1024*1024 if self.path == '/medium' else 0
        data = (marker + '\n').encode() if not count else bytes(range(256))*256
        self.send_response(200); self.send_header('Content-Length', str(count or len(data))); self.end_headers()
        for _ in range(count//len(data) if count else 1): self.wfile.write(data)
        self.wfile.flush()
    def do_POST(self):
        if self.path == '/early':
            self.send_response(413); self.send_header('Content-Length', '0'); self.send_header('Connection', 'close'); self.end_headers(); self.close_connection=True; return
        count = int(self.headers.get('Content-Length','0')); total=0
        while total<count:
            data=self.rfile.read(min(65536,count-total))
            if not data: break
            total+=len(data)
        body=str(total).encode();self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)

def serve(port, tls=False):
    server=http.server.ThreadingHTTPServer(('0.0.0.0',port),Handler)
    if tls:
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain('/var/tmp/sodeck-nt/cert.pem','/var/tmp/sodeck-nt/key.pem');server.socket=context.wrap_socket(server.socket,server_side=True)
    server.serve_forever()
port=int(sys.argv[2]) if len(sys.argv)>2 else 18080
threading.Thread(target=serve,args=(port+363,True),daemon=True).start()
serve(port)
