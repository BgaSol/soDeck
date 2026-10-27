"""Capture server cleanup evidence, then power off only the T-013 QEMU guest."""
import json, time, pathlib, paramiko
root = pathlib.Path(__file__).resolve().parents[3]
out = root / '.qa/t013'
c = paramiko.SSHClient(); c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect('127.0.0.1', port=22313, username='root', key_filename=str(root/'.qa/t008/linux/test-key'), timeout=5)
def run(command):
    _, stdout, stderr = c.exec_command(command, timeout=10)
    result = stdout.read().decode(); code = stdout.channel.recv_exit_status()
    assert code == 0, stderr.read().decode()
    return result
for _ in range(50):
    processes = run('ps -u jump13,target13 -o pid,user,comm --no-headers || true').strip()
    if not processes: break
    time.sleep(.1)
assert not processes, 'Test account processes remain: ' + processes
connections = run('ss -tn state established')
assert ':2222 ' not in connections and ':2200 ' not in connections, connections
for name in ['jump','target']:
    (out/'linux'/f'{name}-sshd.log').write_text(run(f'cat /var/tmp/sodeck-t013/{name}.log'))
evidence = {'serverAccountProcesses':processes,'establishedConnections':connections,'serverCleanupVerified':True}
(out/'cleanup.json').write_text(json.dumps(evidence,indent=2))
c.exec_command('shutdown -h now'); c.close()
print('No jump/target account processes or SSH transports remain; isolated guest shutdown requested.')
