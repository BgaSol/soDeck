"""Resume the existing dedicated T-027 guest after a reboot."""
import time
from pathlib import Path
import paramiko
root = Path(__file__).resolve().parents[3]
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
for attempt in range(60):
    try:
        client.connect('127.0.0.1', port=22373, username='root', key_filename=str(root / '.qa/t008/linux/test-key'), timeout=2, banner_timeout=3)
        break
    except Exception:
        if attempt == 59:
            raise
        time.sleep(1)
command = """set -eu
test -f /var/tmp/sodeck-mr/a.conf
test -f /var/tmp/sodeck-mr/network.sh
mkdir -p /run/sshd
if ! ip netns list | grep -q '^mr-b '; then sh /var/tmp/sodeck-mr/network.sh; fi
for n in a b t; do
  prefix=''; if [ "$n" != a ]; then prefix="ip netns exec mr-$n"; fi
  $prefix /usr/sbin/sshd -t -f /var/tmp/sodeck-mr/$n.conf
  $prefix /usr/sbin/sshd -f /var/tmp/sodeck-mr/$n.conf -E /var/tmp/sodeck-mr/$n.log
done
for port in 12021 12022; do
  nohup ip netns exec mr-b python3 /var/tmp/sodeck-mr/socks.py $port /var/tmp/sodeck-mr/proxy-$port.jsonl </dev/null >/var/tmp/sodeck-mr/proxy-$port.log 2>&1 &
done
nohup python3 /var/tmp/sodeck-mr/socks.py 2223 /var/tmp/sodeck-mr/proxy-2223.jsonl </dev/null >/var/tmp/sodeck-mr/proxy-2223.log 2>&1 &
nohup ip netns exec mr-t python3 -m http.server 8088 --bind 127.0.0.1 --directory /home/mr_t </dev/null >/var/tmp/sodeck-mr/http.log 2>&1 &
"""
_, stdout, stderr = client.exec_command(command, timeout=30)
assert stdout.channel.recv_exit_status() == 0, stderr.read().decode()
client.close()
print('Dedicated T-027 OpenSSH A/B/T, SOCKS, SFTP and HTTP fixtures resumed.')
