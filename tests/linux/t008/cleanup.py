"""Unmount only T-008 fixtures and power off the isolated guest; retain all disks."""
import argparse
import json
import pathlib
import paramiko

parser = argparse.ArgumentParser()
parser.add_argument('--config', required=True)
parser.add_argument('--output', required=True)
args = parser.parse_args()
config = json.loads(pathlib.Path(args.config).read_text())
if config['host'] != '127.0.0.1' or config['port'] != 22280:
    raise SystemExit('isolated guest endpoint required')
ssh = paramiko.SSHClient()
ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
ssh.connect(config['host'], port=config['port'], username='root', key_filename=config['privateKeyPath'])
script = r'''
import json,pathlib,re,subprocess
base='/srv/sodeck-t008'
assert str(pathlib.Path(base).resolve()) == base
def mounts():
 result=[]
 for line in pathlib.Path('/proc/self/mountinfo').read_text().splitlines():
  fields=line.split();path=re.sub(r'\\([0-7]{3})',lambda m:chr(int(m[1],8)),fields[4])
  if path==base or path.startswith(base+'/'):result.append(path)
 return result
before=mounts();removed=[]
for _ in range(32):
 current=mounts()
 if not current:break
 target=max(current,key=lambda path:(path.count('/'),len(path)))
 subprocess.run(['umount','--',target],check=True,capture_output=True)
 removed.append(target)
assert not mounts(), 'fixture mounts remain; keep guest running for investigation'
subprocess.run(['systemctl','stop','smbd'],check=True)
processes=subprocess.run(['pgrep','-a','-u','tester,reader,jailed'],capture_output=True,text=True)
assert processes.returncode==1, 'test account processes remain: '+processes.stdout
print(json.dumps({'mountsBefore':before,'unmounted':removed,'mountsAfter':mounts(),'testAccountProcesses':[],'smbdStopped':True,'filesAndDisksRetained':True}))
'''
try:
    sftp = ssh.open_sftp()
    with sftp.open('/tmp/sodeck-t008-cleanup.py', 'w') as destination:
        destination.write(script)
    sftp.close()
    _, out, err = ssh.exec_command('python3 /tmp/sodeck-t008-cleanup.py', timeout=60)
    stdout, stderr = out.read().decode(), err.read().decode()
    if out.channel.recv_exit_status():
        raise RuntimeError(stderr)
    result = json.loads(stdout)
    _, out, err = ssh.exec_command('systemctl poweroff --no-block', timeout=15)
    if out.channel.recv_exit_status():
        raise RuntimeError(err.read().decode())
    result['poweroffRequested'] = True
    pathlib.Path(args.output).write_text(json.dumps(result, indent=2))
    print(json.dumps(result))
finally:
    ssh.close()
