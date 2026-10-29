"""Create/remove a single byte-path bind mount on the isolated T-024 guest."""
import sys
import paramiko
assert sys.argv[1] in ('setup', 'cleanup')
client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
client.connect('127.0.0.1', port=22342, username='root', key_filename='.qa/t008/linux/test-key')
code = """import os,subprocess
path=b"/var/tmp/sodeck-monitor-special/a b'\\n\\xff"
"""
if sys.argv[1] == 'setup':
    code += "os.makedirs(path,exist_ok=True)\nsubprocess.run([b'mount',b'--bind',b'/var/tmp/sodeck-monitor-load',path],check=True)\n"
else:
    code += "subprocess.run([b'umount',path],check=True)\nos.rmdir(path)\nos.rmdir(b'/var/tmp/sodeck-monitor-special')\n"
try:
    _, out, err = client.exec_command("python3 - <<'T024PY'\n" + code + '\nT024PY')
    assert out.channel.recv_exit_status() == 0, err.read().decode()
    print('T024 special mount ' + sys.argv[1] + ' complete')
finally:
    client.close()
