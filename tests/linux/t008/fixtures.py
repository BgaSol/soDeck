"""Admin-only fixtures inside the isolated T-008 guest; never run on user servers."""
import argparse,json,pathlib,paramiko
parser=argparse.ArgumentParser();parser.add_argument('--config',required=True);args=parser.parse_args()
config=json.loads(pathlib.Path(args.config).read_text())
if config['host']!='127.0.0.1' or config['port']!=22280:raise SystemExit('isolated guest endpoint required')
ssh=paramiko.SSHClient();ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
ssh.connect(config['host'],port=config['port'],username='root',key_filename=config['privateKeyPath'])
script=r"""
set -eu
base=/srv/sodeck-t008
mkdir -p "$base"/{ordinary,empty,many,source,bind,readonly,tmpfs,nested,network,stack,btrfs,subvolume,denied,chroot}
chmod 755 "$base" "$base/chroot"
chmod 700 "$base/denied"
python3 - <<'PY'
import os,socket
base=b'/srv/sodeck-t008/ordinary'
for name,size in [(b'zero',0),(b'1023.bin',1023),(b'1024.bin',1024),(b'.hidden',12),(b'back\\slash',33),(b'invalid-\xff',44),(b"space quote' $(do-not-run)\n",55),(b'sparse-5GiB.bin',5*1024**3)]:
 with open(base+b'/'+name,'wb') as f:f.truncate(size)
for name,target in [(b'link-file',b'1024.bin'),(b'broken',b'missing'),(b'loop',b'loop'),(b'link-dir',b'../source')]:
 try:os.symlink(target,base+b'/'+name)
 except FileExistsError:pass
try:os.mkfifo(base+b'/pipe')
except FileExistsError:pass
for n in range(10000):
 with open('/srv/sodeck-t008/many/item-%05d.dat'%n,'wb') as f:f.truncate(n)
with open('/srv/sodeck-t008/source/shared.txt','w') as f:f.write('Shared through bind\n')
with open('/srv/sodeck-t008/denied/private','w') as f:f.write('unreadable to test users')
PY
mountpoint -q "$base/bind" || mount --bind "$base/source" "$base/bind"
mountpoint -q "$base/readonly" || { mount --bind "$base/source" "$base/readonly"; mount -o remount,bind,ro "$base/readonly"; }
mountpoint -q "$base/tmpfs" || mount -t tmpfs -o size=32m tmpfs "$base/tmpfs"
mkdir -p "$base/tmpfs/inside"
mountpoint -q "$base/tmpfs/inside" || mount -t tmpfs -o size=4m,ro tmpfs "$base/tmpfs/inside"
if ! mountpoint -q "$base/stack"; then mount -t tmpfs -o size=8m lower "$base/stack"; mount -t tmpfs -o size=4m upper "$base/stack"; fi
if [ ! -f "$base/btrfs.img" ]; then truncate -s 256M "$base/btrfs.img"; mkfs.btrfs "$base/btrfs.img" >/dev/null; fi
mountpoint -q "$base/btrfs" || mount -o loop "$base/btrfs.img" "$base/btrfs"
[ -d "$base/btrfs/example" ] || btrfs subvolume create "$base/btrfs/example"
mountpoint -q "$base/subvolume" || mount -o loop,subvol=example "$base/btrfs.img" "$base/subvolume"
cat > /etc/samba/smb.conf <<'CONF'
[global]
interfaces = lo
bind interfaces only = yes
map to guest = Bad User
server min protocol = SMB2
[t008]
path = /srv/sodeck-t008/source
guest ok = yes
read only = yes
CONF
systemctl restart smbd
mountpoint -q "$base/network" || mount -t cifs //127.0.0.1/t008 "$base/network" -o guest,ro,vers=3.0
id jailed >/dev/null 2>&1 || useradd -m -s /bin/sh jailed
install -d -m 700 -o jailed -g jailed /home/jailed/.ssh
install -m 600 -o jailed -g jailed /home/tester/.ssh/authorized_keys /home/jailed/.ssh/authorized_keys
mkdir -p "$base/chroot/data"; printf 'jailed\n' > "$base/chroot/data/only-inside"
cat > /etc/ssh/sshd_config.d/t008-jailed.conf <<'CONF'
Match User jailed
  ChrootDirectory /srv/sodeck-t008/chroot
  ForceCommand internal-sftp
Match all
CONF
sshd -t && systemctl reload ssh
findmnt -R /srv/sodeck-t008 || true
free -m
"""
try:
    sftp=ssh.open_sftp()
    with sftp.open('/tmp/sodeck-t008-fixtures.sh','w') as f:f.write(script)
    _,out,err=ssh.exec_command('bash /tmp/sodeck-t008-fixtures.sh',timeout=240)
    print(out.read().decode());print(err.read().decode());status=out.channel.recv_exit_status()
    if status:raise SystemExit(status)
finally:ssh.close()
