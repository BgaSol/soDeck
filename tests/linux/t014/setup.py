"""Dedicated QEMU guest only: A -> namespace B -> namespace C -> namespace D."""
from pathlib import Path
import json
import time
import paramiko

root = Path(__file__).resolve().parents[3]
out = root / '.qa/t014/linux'
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

run('mkdir -p /run/sshd /var/tmp/sodeck-nt; command -v ip; command -v ssh-keygen')
sftp = client.open_sftp()
script = '''set -eu
for n in b c d; do ip netns add nt-$n; ip -n nt-$n link set lo up; mkdir -p /etc/netns/nt-$n; done
ip link add nt-a type veth peer name nt-b0
ip link set nt-b0 netns nt-b
ip addr add 10.213.1.1/30 dev nt-a
ip link set nt-a up
ip -n nt-b addr add 10.213.1.2/30 dev nt-b0
ip -n nt-b link set nt-b0 up
ip link add nt-b1 type veth peer name nt-c0
ip link set nt-b1 netns nt-b
ip link set nt-c0 netns nt-c
ip -n nt-b addr add 10.213.2.1/30 dev nt-b1
ip -n nt-b link set nt-b1 up
ip -n nt-c addr add 10.213.2.2/30 dev nt-c0
ip -n nt-c link set nt-c0 up
ip link add nt-c1 type veth peer name nt-d0
ip link set nt-c1 netns nt-c
ip link set nt-d0 netns nt-d
ip -n nt-c addr add 10.213.3.1/30 dev nt-c1
ip -n nt-c link set nt-c1 up
ip -n nt-d addr add 10.213.3.2/30 dev nt-d0
ip -n nt-d link set nt-d0 up
for n in b c d; do ip netns exec nt-$n sysctl -qw net.ipv4.ip_forward=0; done
printf '\n10.213.1.2 node-b.t014.invalid\n' >> /etc/hosts
printf '127.0.0.1 localhost\n10.213.2.2 node-c.t014.invalid\n' > /etc/netns/nt-b/hosts
printf '127.0.0.1 localhost\n10.213.3.2 node-d.t014.invalid\n' > /etc/netns/nt-c/hosts
printf '127.0.0.1 localhost\n' > /etc/netns/nt-d/hosts
'''
with sftp.open('/var/tmp/sodeck-nt/network.sh', 'w') as stream:
    stream.write(script)
run('sh /var/tmp/sodeck-nt/network.sh')
nodes = []
for index, name in enumerate('abcd'):
    user = f'nt_{name}'
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
    run(f'chmod 600 /home/{user}/.ssh/authorized_keys; chown -R {user}:{user} /home/{user}; ssh-keygen -q -t ed25519 -N "" -f /var/tmp/sodeck-nt/{name}-host')
    node = {'name': name.upper(), 'host': '127.0.0.1' if index == 0 else f'node-{name}.t014.invalid', 'port': 22324 if index == 0 else 22, 'username': user, 'key': str(key_path), 'encryptedKey': str(encrypted_path)}
    nodes.append(node)
    modes = [('jump', 2222 if index == 0 else 22, 0 if index < 3 else 20)]
    if index in (1, 2):
        modes.append(('target', 2224, 20))
    for mode, port, sessions in modes:
        config = f'''Port {port}
ListenAddress 0.0.0.0
HostKey /var/tmp/sodeck-nt/{name}-host
PidFile /var/tmp/sodeck-nt/{name}-{mode}.pid
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
        conf = f'/var/tmp/sodeck-nt/{name}-{mode}.conf'
        with sftp.open(conf, 'w') as stream:
            stream.write(config)
        prefix = '' if index == 0 else f'ip netns exec nt-{name} '
        run(f'{prefix}/usr/sbin/sshd -t -f {conf}; {prefix}/usr/sbin/sshd -f {conf} -E /var/tmp/sodeck-nt/{name}-{mode}.log')

evidence = run('''/usr/sbin/sshd -V 2>&1
ip netns list
ip route
for n in b c d; do echo "NAMESPACE $n"; ip -n nt-$n route; ip netns exec nt-$n cat /etc/hosts; ip netns exec nt-$n sysctl net.ipv4.ip_forward; done
getent hosts node-b.t014.invalid
! getent hosts node-c.t014.invalid
! ip netns exec nt-b getent hosts node-d.t014.invalid
for n in a b c; do /usr/sbin/sshd -T -f /var/tmp/sodeck-nt/$n-jump.conf | grep maxsessions; done
''')
(out / 'topology.txt').write_text(evidence)
(out / 'config.json').write_text(json.dumps({'adminHost': '127.0.0.1', 'adminPort': 22323, 'nodes': nodes}, indent=2))
run('''ip netns add nt-e; ip netns add nt-f
ip -n nt-e link set lo up; ip -n nt-f link set lo up
ip -n nt-d link add nt-lan type bridge
ip -n nt-d addr add 198.18.14.16/24 dev nt-lan
ip -n nt-d link set nt-lan up
for n in e f; do
  ip link add nt-$n-d type veth peer name nt-$n-0
  ip link set nt-$n-d netns nt-d; ip link set nt-$n-0 netns nt-$n
  ip -n nt-d link set nt-$n-d master nt-lan; ip -n nt-d link set nt-$n-d up
  ip -n nt-$n link set nt-$n-0 up
done
ip -n nt-e addr add 198.18.14.18/24 dev nt-e-0
ip -n nt-f addr add 198.18.14.19/24 dev nt-f-0
printf '127.0.0.1 localhost\n198.18.14.18 service-e.t014.invalid\n198.18.14.19 service-f.t014.invalid\n' > /etc/netns/nt-d/hosts
openssl req -x509 -newkey rsa:2048 -nodes -keyout /var/tmp/sodeck-nt/key.pem -out /var/tmp/sodeck-nt/cert.pem -days 2 -subj /CN=service-e.t014.invalid -addext subjectAltName=DNS:service-e.t014.invalid,DNS:service-f.t014.invalid,IP:198.18.14.18,IP:198.18.14.19 >/dev/null 2>&1
''')
sftp.put(str(Path(__file__).with_name('services.py')), '/var/tmp/sodeck-nt/services.py')
run('nohup python3 /var/tmp/sodeck-nt/services.py REMOTE-A 18082 >/var/tmp/sodeck-nt/benchmark-http.log 2>&1 </dev/null &')
run('ip addr add 198.18.14.18/32 dev lo; nohup python3 /var/tmp/sodeck-nt/services.py REMOTE-A-ISOLATED 18080 >/var/tmp/sodeck-nt/same-address-http.log 2>&1 </dev/null &')
for ns, marker in [('nt-d','REMOTE-D-LOOPBACK'),('nt-e','REMOTE-E'),('nt-f','REMOTE-F')]:
    run(f'ip netns exec {ns} sh -c "nohup python3 /var/tmp/sodeck-nt/services.py {marker} >/var/tmp/sodeck-nt/{ns}-http.log 2>&1 </dev/null &"')
sftp.get('/var/tmp/sodeck-nt/cert.pem',str(out/'cert.pem'))
sftp.close()
client.close()
print('A/B/C/D OpenSSH namespace chain ready; forwarding disabled between nonadjacent nodes.')

