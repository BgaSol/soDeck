"""Dedicated QEMU guest only: A -> namespace B -> namespace C -> namespace D."""
from pathlib import Path
import json
import time
import paramiko

root = Path(__file__).resolve().parents[3]
out = root / '.qa/t013-multihop/linux'
out.mkdir(parents=True, exist_ok=True)
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
for attempt in range(60):
    try:
        client.connect('127.0.0.1', port=22323, username='root', key_filename=str(root / '.qa/t008/linux/test-key'), timeout=3, banner_timeout=5)
        break
    except Exception:
        if attempt == 59:
            raise
        time.sleep(1)

def run(command):
    _, stdout, stderr = client.exec_command(command, timeout=30)
    text = stdout.read().decode()
    assert stdout.channel.recv_exit_status() == 0, stderr.read().decode()
    return text

run('mkdir -p /run/sshd /var/tmp/sodeck-mh; command -v ip; command -v ssh-keygen')
sftp = client.open_sftp()
script = '''set -eu
for n in b c d; do ip netns add mh-$n; ip -n mh-$n link set lo up; mkdir -p /etc/netns/mh-$n; done
ip link add mh-a type veth peer name mh-b0
ip link set mh-b0 netns mh-b
ip addr add 10.213.1.1/30 dev mh-a
ip link set mh-a up
ip -n mh-b addr add 10.213.1.2/30 dev mh-b0
ip -n mh-b link set mh-b0 up
ip link add mh-b1 type veth peer name mh-c0
ip link set mh-b1 netns mh-b
ip link set mh-c0 netns mh-c
ip -n mh-b addr add 10.213.2.1/30 dev mh-b1
ip -n mh-b link set mh-b1 up
ip -n mh-c addr add 10.213.2.2/30 dev mh-c0
ip -n mh-c link set mh-c0 up
ip link add mh-c1 type veth peer name mh-d0
ip link set mh-c1 netns mh-c
ip link set mh-d0 netns mh-d
ip -n mh-c addr add 10.213.3.1/30 dev mh-c1
ip -n mh-c link set mh-c1 up
ip -n mh-d addr add 10.213.3.2/30 dev mh-d0
ip -n mh-d link set mh-d0 up
for n in b c d; do ip netns exec mh-$n sysctl -qw net.ipv4.ip_forward=0; done
printf '\n10.213.1.2 node-b.t013m.invalid\n' >> /etc/hosts
printf '127.0.0.1 localhost\n10.213.2.2 node-c.t013m.invalid\n' > /etc/netns/mh-b/hosts
printf '127.0.0.1 localhost\n10.213.3.2 node-d.t013m.invalid\n' > /etc/netns/mh-c/hosts
printf '127.0.0.1 localhost\n' > /etc/netns/mh-d/hosts
'''
with sftp.open('/var/tmp/sodeck-mh/network.sh', 'w') as stream:
    stream.write(script)
run('sh /var/tmp/sodeck-mh/network.sh')
nodes = []
for index, name in enumerate('abcd'):
    user = f'mh_{name}'
    run(f'id {user} >/dev/null 2>&1 || useradd -m -s /bin/bash {user}')
    run(f"printf '{user}:multihop-{name}-fixture\n' | chpasswd")
    key = paramiko.RSAKey.generate(2048)
    key_path = out / f'{name}-key'
    encrypted_path = out / f'{name}-encrypted-key'
    key.write_private_key_file(str(key_path))
    key.write_private_key_file(str(encrypted_path), password='fixture-passphrase')
    run(f'mkdir -p /home/{user}/.ssh /home/{user}/proof; chmod 700 /home/{user}/.ssh')
    with sftp.open(f'/home/{user}/.ssh/authorized_keys', 'w') as stream:
        stream.write(key.get_name() + ' ' + key.get_base64() + '\n')
    with sftp.open(f'/home/{user}/proof/node.txt', 'w') as stream:
        stream.write(f'OpenSSH node {name}\n')
    run(f'chmod 600 /home/{user}/.ssh/authorized_keys; chown -R {user}:{user} /home/{user}; ssh-keygen -q -t ed25519 -N "" -f /var/tmp/sodeck-mh/{name}-host')
    node = {'name': name.upper(), 'host': '127.0.0.1' if index == 0 else f'node-{name}.t013m.invalid', 'port': 22324 if index == 0 else 22, 'username': user, 'key': str(key_path), 'encryptedKey': str(encrypted_path)}
    nodes.append(node)
    modes = [('jump', 2222 if index == 0 else 22, 0 if index < 3 else 20)]
    if index in (1, 2):
        modes.append(('target', 2224, 20))
    for mode, port, sessions in modes:
        config = f'''Port {port}
ListenAddress 0.0.0.0
HostKey /var/tmp/sodeck-mh/{name}-host
PidFile /var/tmp/sodeck-mh/{name}-{mode}.pid
AuthorizedKeysFile .ssh/authorized_keys
PasswordAuthentication yes
KbdInteractiveAuthentication no
UsePAM no
AllowUsers {user}
AllowTcpForwarding yes
MaxSessions {sessions}
MaxStartups 100:30:200
LogLevel DEBUG1
Subsystem sftp internal-sftp
'''
        conf = f'/var/tmp/sodeck-mh/{name}-{mode}.conf'
        with sftp.open(conf, 'w') as stream:
            stream.write(config)
        prefix = '' if index == 0 else f'ip netns exec mh-{name} '
        run(f'{prefix}/usr/sbin/sshd -t -f {conf}; {prefix}/usr/sbin/sshd -f {conf} -E /var/tmp/sodeck-mh/{name}-{mode}.log')

evidence = run('''/usr/sbin/sshd -V 2>&1
ip netns list
ip route
for n in b c d; do echo "NAMESPACE $n"; ip -n mh-$n route; ip netns exec mh-$n cat /etc/hosts; ip netns exec mh-$n sysctl net.ipv4.ip_forward; done
getent hosts node-b.t013m.invalid
! getent hosts node-c.t013m.invalid
! ip netns exec mh-b getent hosts node-d.t013m.invalid
for n in a b c; do /usr/sbin/sshd -T -f /var/tmp/sodeck-mh/$n-jump.conf | grep maxsessions; done
''')
(out / 'topology.txt').write_text(evidence)
(out / 'config.json').write_text(json.dumps({'adminHost': '127.0.0.1', 'adminPort': 22323, 'nodes': nodes}, indent=2))
sftp.close()
client.close()
print('A/B/C/D OpenSSH namespace chain ready; forwarding disabled between nonadjacent nodes.')
