"""Execute bounded test input only on the dedicated T-024 QEMU endpoint."""
import base64
import json
import sys
from pathlib import Path
import paramiko

root = Path(__file__).resolve().parents[3]
request = json.load(sys.stdin)
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect('127.0.0.1', port=22342, username='tester', key_filename=str(root / '.qa/t008/linux/test-key'), timeout=10)
try:
    channel = client.get_transport().open_session(timeout=5)
    channel.settimeout(10)
    channel.exec_command('/bin/busybox sh -s' if request.get('busybox') else 'sh -s')
    channel.sendall(base64.b64decode(request['stdin']))
    channel.shutdown_write()
    output = channel.makefile('rb').read(600000)
    error = channel.makefile_stderr('rb').read(65536)
    status = channel.recv_exit_status()
    print(json.dumps({'stdout':base64.b64encode(output).decode(), 'stderr':base64.b64encode(error).decode(), 'exitCode':status}))
finally:
    client.close()
