"""Whole isolated-guest counters; not process attribution or production benchmarks."""
import json
import sys
import time
from pathlib import Path
import paramiko
assert sys.argv[1] in ('controlled-load', 'idle-after-load')
c=paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect('127.0.0.1',port=22342,username='tester',key_filename='.qa/t008/linux/test-key')
def sample():
    _,out,err=c.exec_command('head -n 1 /proc/stat; cat /proc/uptime; cat /proc/net/dev',timeout=5)
    lines=out.read().decode().splitlines()
    assert out.channel.recv_exit_status()==0,err.read().decode()
    cpu=list(map(int,lines[0].split()[1:9]));net={}
    for line in lines[2:]:
        if ':' not in line:continue
        name,values=line.split(':',1);fields=values.split()
        net[name.strip()]=[int(fields[0]),int(fields[8])]
    return {'cpu':cpu,'uptime':float(lines[1].split()[0]),'network':net}
try:
    measured_at=int(time.time()*1000)
    progress=json.loads(Path('.qa/t024/load/progress.json').read_text(encoding='utf-8'))
    before=sample();time.sleep(10);after=sample();seconds=after['uptime']-before['uptime'];delta=[b-a for a,b in zip(before['cpu'],after['cpu'])]
    result={'label':sys.argv[1],'measuredAt':measured_at,'loadStart':progress['start'],'scope':'whole dedicated Linux guest; includes SSH, PTY, SFTP, HTTP and controlled monitor scripts','durationSeconds':seconds,'cpuBusyPercent':100*(sum(delta)-delta[3]-delta[4])/sum(delta),'networkBytesPerSecond':{name:{'rx':(values[0]-before['network'][name][0])/seconds,'tx':(values[1]-before['network'][name][1])/seconds} for name,values in after['network'].items() if name in before['network']},'before':before,'after':after}
    path=Path('.qa/t024/load')/(sys.argv[1]+'-guest.json');path.write_text(json.dumps(result,indent=2));print(json.dumps({k:v for k,v in result.items() if k not in ('before','after')}))
finally:c.close()
