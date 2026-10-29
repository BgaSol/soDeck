"""Prepare only the dedicated T-027 QEMU guest at loopback port 22373."""
import json
import time
from pathlib import Path
import paramiko

root = Path(__file__).resolve().parents[3]
out = root / '.qa/t027/linux'
out.mkdir(parents=True, exist_ok=True)
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
for attempt in range(45):
    try:
        client.connect('127.0.0.1', port=22373, username='root', key_filename=str(root / '.qa/t008/linux/test-key'), timeout=2, banner_timeout=3)
        break
    except Exception:
        if attempt == 44:
            raise
        time.sleep(1)


def run(command):
    _, stdout, stderr = client.exec_command(command, timeout=30)
    text = stdout.read().decode()
    assert stdout.channel.recv_exit_status() == 0, stderr.read().decode()
    return text


sftp = client.open_sftp()
run('mkdir -p /run/sshd /var/tmp/sodeck-mr; command -v python3; command -v ip')
network = '''set -eu
for n in b t; do ip netns add mr-$n; ip -n mr-$n link set lo up; mkdir -p /etc/netns/mr-$n; done
ip link add mr-a type veth peer name mr-b0
ip link set mr-b0 netns mr-b
ip addr add 10.227.1.1/30 dev mr-a
ip link set mr-a up
ip -n mr-b addr add 10.227.1.2/30 dev mr-b0
ip -n mr-b link set mr-b0 up
ip link add mr-b1 type veth peer name mr-t0
ip link set mr-b1 netns mr-b
ip link set mr-t0 netns mr-t
ip -n mr-b addr add 10.227.2.1/30 dev mr-b1
ip -n mr-b link set mr-b1 up
ip -n mr-t addr add 10.227.2.2/30 dev mr-t0
ip -n mr-t link set mr-t0 up
for n in b t; do ip netns exec mr-$n sysctl -qw net.ipv4.ip_forward=0; done
printf '\n10.227.1.2 b.t027.invalid\n' >> /etc/hosts
printf '127.0.0.1 localhost\n10.227.2.2 target.t027.invalid\n' > /etc/netns/mr-b/hosts
printf '127.0.0.1 localhost\n' > /etc/netns/mr-t/hosts
'''
with sftp.open('/var/tmp/sodeck-mr/network.sh', 'w') as stream:
    stream.write(network)
run('sh /var/tmp/sodeck-mr/network.sh')
nodes = []
for index, name in enumerate('abt'):
    user = f'mr_{name}'
    run(f'useradd -m -s /bin/bash {user}; printf "{user}:isolated-t027-{name}\\n" | chpasswd')
    key = paramiko.RSAKey.generate(2048)
    key_path = out / f'{name}-key'
    key.write_private_key_file(str(key_path))
    run(f'mkdir -p /home/{user}/.ssh; chmod 700 /home/{user}/.ssh')
    with sftp.open(f'/home/{user}/.ssh/authorized_keys', 'w') as stream:
        stream.write(key.get_name() + ' ' + key.get_base64() + '\n')
    run(f'chmod 600 /home/{user}/.ssh/authorized_keys; chown -R {user}:{user} /home/{user}; ssh-keygen -q -t ed25519 -N "" -f /var/tmp/sodeck-mr/{name}-host')
    conf = f'/var/tmp/sodeck-mr/{name}.conf'
    with sftp.open(conf, 'w') as stream:
        stream.write(f'''Port {2222 if name == 'a' else 22}
ListenAddress 0.0.0.0
HostKey /var/tmp/sodeck-mr/{name}-host
PidFile /var/tmp/sodeck-mr/{name}.pid
AuthorizedKeysFile .ssh/authorized_keys
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
AllowUsers {user}
AllowTcpForwarding yes
MaxSessions {20 if name == 't' else 0}
LogLevel DEBUG1
Subsystem sftp internal-sftp
''')
    prefix = '' if name == 'a' else f'ip netns exec mr-{name} '
    run(f'{prefix}/usr/sbin/sshd -t -f {conf}; {prefix}/usr/sbin/sshd -f {conf} -E /var/tmp/sodeck-mr/{name}.log')
    nodes.append(dict(kind='ssh', name=name, host='127.0.0.1' if name == 'a' else ('b.t027.invalid' if name == 'b' else 'target.t027.invalid'), port=22374 if name == 'a' else 22, username=user, key=str(key_path)))
sftp.put(str(Path(__file__).with_name('socks.py')), '/var/tmp/sodeck-mr/socks.py')
for prefix, port in [('ip netns exec mr-b ', 12021), ('ip netns exec mr-b ', 12022), ('', 2223)]:
    run(f'nohup {prefix}python3 /var/tmp/sodeck-mr/socks.py {port} /var/tmp/sodeck-mr/proxy-{port}.jsonl </dev/null >/var/tmp/sodeck-mr/proxy-{port}.log 2>&1 &')
proxy = lambda port: dict(kind='socks', host='127.0.0.1', port=port)
a, b, target = nodes
paths = [[a, b, proxy(12021), target], [a, b, proxy(12021), proxy(12022), target], [proxy(22375), b, proxy(12021), target]]
(out / 'config.json').write_text(json.dumps(dict(paths=paths), indent=2))
evidence = run('''/usr/sbin/sshd -V 2>&1
ip netns list
! getent hosts target.t027.invalid
ip netns exec mr-b getent hosts target.t027.invalid
ip netns exec mr-b ss -lntp
for n in a b; do /usr/sbin/sshd -T -f /var/tmp/sodeck-mr/$n.conf | grep maxsessions; done
''')
(out / 'topology.txt').write_text(evidence)
sftp.close()
client.close()
print('T-027 A/B/target OpenSSH and B-loopback SOCKS fixtures ready.')
