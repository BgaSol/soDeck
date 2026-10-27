"""Isolated QEMU topology: host :22314 -> gateway :2222 -> target.t013:2200.

The admin endpoint :22313 is fixture-only. The target binds guest loopback,
has no QEMU host forward, and its name exists only in the guest hosts file.
"""
from pathlib import Path
import json, time, paramiko

root = Path(__file__).resolve().parents[3]
out = root / '.qa/t013/linux'
out.mkdir(parents=True, exist_ok=True)
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
for attempt in range(60):
    try:
        client.connect('127.0.0.1', port=22313, username='root', key_filename=str(root / '.qa/t008/linux/test-key'), timeout=3, banner_timeout=5)
        break
    except Exception:
        if attempt == 59: raise
        time.sleep(1)

def run(command):
    _, stdout, stderr = client.exec_command(command, timeout=20)
    text = stdout.read().decode()
    assert stdout.channel.recv_exit_status() == 0, stderr.read().decode()
    return text

keys = {}
for name in ['jump', 'target']:
    key = paramiko.RSAKey.generate(2048)
    key.write_private_key_file(str(out / (name + '-key')))
    key.write_private_key_file(str(out / (name + '-encrypted-key')), password='fixture-passphrase')
    keys[name] = key.get_name() + ' ' + key.get_base64()
run('mkdir -p /run/sshd /var/tmp/sodeck-t013; id jump13 >/dev/null 2>&1 || useradd -m -s /usr/sbin/nologin jump13; id target13 >/dev/null 2>&1 || useradd -m -s /bin/bash target13')
run("printf 'jump13:jump-fixture-password\ntarget13:target-fixture-password\n' | chpasswd")
sftp = client.open_sftp()
for name in keys:
    user = name + '13'
    run(f'mkdir -p /home/{user}/.ssh; chmod 700 /home/{user}/.ssh')
    with sftp.open(f'/home/{user}/.ssh/authorized_keys', 'w') as f: f.write(keys[name] + '\n')
    run(f'chmod 600 /home/{user}/.ssh/authorized_keys; chown -R {user}:{user} /home/{user}/.ssh')
    run(f'test -f /var/tmp/sodeck-t013/{name}-host || ssh-keygen -q -t ed25519 -N "" -f /var/tmp/sodeck-t013/{name}-host')
    config = f'''Port {2222 if name == 'jump' else 2200}
ListenAddress {'0.0.0.0' if name == 'jump' else '127.0.0.2'}
HostKey /var/tmp/sodeck-t013/{name}-host
PidFile /var/tmp/sodeck-t013/{name}.pid
AuthorizedKeysFile .ssh/authorized_keys
PasswordAuthentication yes
KbdInteractiveAuthentication no
UsePAM no
AllowUsers {user}
AllowTcpForwarding {'yes' if name == 'jump' else 'no'}
MaxSessions {0 if name == 'jump' else 20}
LogLevel DEBUG1
Subsystem sftp internal-sftp
'''
    with sftp.open(f'/var/tmp/sodeck-t013/{name}.conf', 'w') as f: f.write(config)
    run(f'/usr/sbin/sshd -t -f /var/tmp/sodeck-t013/{name}.conf; /usr/sbin/sshd -f /var/tmp/sodeck-t013/{name}.conf -E /var/tmp/sodeck-t013/{name}.log')
run("printf '\n127.0.0.2 target.t013\n' >> /etc/hosts; mkdir -p /home/target13/jump-proof; printf 'T013 file proof\n' > /home/target13/jump-proof/proof.txt; chown -R target13:target13 /home/target13/jump-proof")
evidence = run("/usr/sbin/sshd -V 2>&1; getent hosts target.t013; ss -lntp | head -20; /usr/sbin/sshd -T -f /var/tmp/sodeck-t013/jump.conf | grep maxsessions")
(out / 'topology.txt').write_text(evidence)
(out / 'config.json').write_text(json.dumps({'host':'127.0.0.1','port':22314,'targetHost':'target.t013','targetPort':2200,'jumpUser':'jump13','targetUser':'target13','jumpKey':str(out/'jump-key'),'targetKey':str(out/'target-key'),'jumpEncryptedKey':str(out/'jump-encrypted-key'),'targetEncryptedKey':str(out/'target-encrypted-key')}))
sftp.close(); client.close()
print('Isolated OpenSSH gateway/target ready; MaxSessions=0 on gateway.')
