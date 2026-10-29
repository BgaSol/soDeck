"""Target-only read/HTTP fixtures inside the dedicated T-027 VM."""
from pathlib import Path
import paramiko

root = Path(__file__).resolve().parents[3]
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect('127.0.0.1', port=22373, username='root', key_filename=str(root / '.qa/t008/linux/test-key'), timeout=5)
command = """printf 'T027_FINAL_TARGET\n' > /home/mr_t/t027-marker.txt
chown mr_t:mr_t /home/mr_t/t027-marker.txt
nohup ip netns exec mr-t python3 -m http.server 8088 --bind 127.0.0.1 --directory /home/mr_t </dev/null >/var/tmp/sodeck-mr/http.log 2>&1 &
"""
_, stdout, stderr = client.exec_command(command)
assert stdout.channel.recv_exit_status() == 0, stderr.read().decode()
client.close()
print('Target-only file and HTTP service prepared')
