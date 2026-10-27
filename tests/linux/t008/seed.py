"""Prepare a NoCloud seed for the isolated T-008 VM; never target an existing server."""
from pathlib import Path
import io, json, sys
import paramiko, pycdlib

root = Path(sys.argv[1]).resolve()
assert root.name == 'linux' and root.parent.name == 't008'
root.mkdir(parents=True, exist_ok=True)
keyfile = root / 'test-key'
if not keyfile.exists():
    paramiko.RSAKey.generate(3072).write_private_key_file(str(keyfile))
key = paramiko.RSAKey.from_private_key_file(str(keyfile))
public = f'{key.get_name()} {key.get_base64()}'
user_data = f'''#cloud-config
hostname: sodeck-t008
manage_etc_hosts: true
ssh_pwauth: false
disable_root: false
users:
  - name: root
    ssh_authorized_keys: [{public}]
  - name: tester
    shell: /bin/bash
    lock_passwd: true
    ssh_authorized_keys: [{public}]
  - name: reader
    shell: /bin/bash
    lock_passwd: true
    ssh_authorized_keys: [{public}]
package_update: true
packages: [openssh-server, btrfs-progs, samba, cifs-utils, python3]
runcmd:
  - [sh, -c, 'echo sodeck-t008-ready > /var/tmp/sodeck-t008-ready']
'''
iso = pycdlib.PyCdlib()
iso.new(interchange_level=3, joliet=3, rock_ridge='1.09', vol_ident='cidata')
for name, content in [('user-data', user_data), ('meta-data', 'instance-id: sodeck-t008-v1\nlocal-hostname: sodeck-t008\n')]:
    data = content.encode()
    iso.add_fp(io.BytesIO(data), len(data), iso_path='/' + name.upper().replace('-', '_') + ';1', rr_name=name, joliet_path='/' + name)
iso.write(str(root / 'seed.iso')); iso.close()
(root / 'test-config.json').write_text(json.dumps({'host':'127.0.0.1','port':22280,'username':'tester','privateKeyPath':str(keyfile),'secondUsername':'reader'}))
print('Isolated seed and private test configuration prepared')
