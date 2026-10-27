"""Actual SSH login shell compatibility, confined to dedicated T010 accounts."""
import base64,json,pathlib,re,sys,time,shlex
from shell_gate import connect,command
TAG='12345678-1234-1234-1234-123456789abc'
PATTERN=rb'\x1b\]633;SoDeck;1;'+TAG.encode()+rb';([^\x1b\x07]+)\x1b\\'
def receive(ch,ready=True,timeout=15):
    data=b'';end=time.monotonic()+timeout
    while time.monotonic()<end:
        if ch.recv_ready():
            data+=ch.recv(65536)
            if ready and any(b';ready;' in f for f in re.findall(PATTERN,data)):return data
        if ch.closed:break
        time.sleep(.02)
    if ready:raise AssertionError('No ready frame: '+repr(data[-1500:]))
    return data
def report(data):
    fields=[f.split(b';') for f in re.findall(PATTERN,data) if b';ready;' in f][-1]
    return {'sequence':int(fields[0]),'pid':fields[2].decode(),'start':fields[3].decode(),'logical':base64.b64decode(fields[8]),'physical':base64.b64decode(fields[9])[:-1]}
def send(ch,text):ch.send(text+b'\r');return receive(ch)
def run(config,bootstrap,output):
    assert config['host']=='127.0.0.1' and config['port']==22281
    root=connect(config,'root');results=[]
    for name,shell in [('b51','/opt/bash51/bin/bash'),('b52','/bin/bash'),('zsh','/bin/zsh')]:
        user='sodeck-t010-'+name;home='/home/'+user
        command(root,f'id {user} >/dev/null 2>&1 || useradd -m -s {shell} {user}; install -d -o {user} -g {user} -m700 {home}/.ssh; install -o {user} -g {user} -m600 /root/.ssh/authorized_keys {home}/.ssh/authorized_keys')
        sf=root.open_sftp()
        for file,content in {'.bash_profile':'. ~/.bashrc\n','.bashrc':"case $- in *i*) ;; *) return;; esac\nPS1='T010_READY> '\nHISTFILE=~/.t010_history\n",'.zshrc':"PS1='T010_READY> '\n"}.items():sf.open(home+'/'+file,'w').write(content)
        command(root,f'chown {user}:{user} {home}/.bash_profile {home}/.bashrc {home}/.zshrc')
        client=connect(config,user);ch=client.get_transport().open_session();ch.get_pty(width=160,height=40);ch.exec_command(bootstrap)
        if name=='zsh':
            data=receive(ch,False,2);assert b'SoDeck;1;' not in data and b'T010_READY>' in data
            ch.send(b"printf 'ZSH_OK:%s\\n' $ZSH_VERSION\r");data+=receive(ch,False,1);assert b'ZSH_OK:5.9' in data
            results.append({'shell':'Zsh 5.9','result':'ordinary interactive terminal; no directory reports'});ch.close();client.close();continue
        initial=report(receive(ch));assert initial['physical']==home.encode()
        data=send(ch,b"shopt -q login_shell; printf 'LOGIN:%s VERSION:%s\\n' \"$?\" \"$BASH_VERSION\"");assert b'LOGIN:0 VERSION:5.' in data
        previous=initial
        for text,target in [(b'cd /tmp',b'/tmp'),(b'cd /no-t010-directory',b'/tmp'),(b'cd -',home.encode()),(b'pushd /etc',b'/etc'),(b'popd',home.encode()),(b"alias t010cd='cd /var'; t010cd",home.encode()),(b't010cd',b'/var'),(b'f(){ cd /etc; }; f',b'/etc'),(b'cd /tmp; cd /usr',b'/usr')]:
            data=send(ch,text);current=report(data);assert current['physical']==target,(text,current)
            assert current['pid']==initial['pid'] and current['start']==initial['start'] and current['sequence']>previous['sequence'];previous=current
        send(ch,b'false | true');data=send(ch,b"printf 'STATUS:%s PIPE:%s\\n' \"$?\" \"${PIPESTATUS[*]}\"");assert b'STATUS:0 PIPE:1 0' in data,data
        send(ch,b'false');data=send(ch,b"printf 'EXIT:%s\\n' \"$?\"");assert b'EXIT:1' in data,data
        raw=b'/tmp/sodeck-t010-'+name.encode()+b'/bytes-\xff\n\n'
        setup=b"mkdir -p $'/tmp/sodeck-t010-"+name.encode()+b"/bytes-\\xff\\n\\n'; cd $'/tmp/sodeck-t010-"+name.encode()+b"/bytes-\\xff\\n\\n'"
        current=report(send(ch,setup));assert current['logical']==raw and current['physical']==raw
        data=send(ch,b"ln -sfn /etc ~/t010-link; cd ~/t010-link");current=report(data);assert current['physical']==b'/etc' and current['logical']==home.encode()+b'/t010-link'
        ch.send(b'bash --noprofile --norc -i\r');data=receive(ch,False,.7);assert not any(b';ready;' in f for f in re.findall(PATTERN,data))
        ch.send(b'cd /tmp\r');data=receive(ch,False,.5);assert not any(b';ready;' in f for f in re.findall(PATTERN,data))
        current=report(send(ch,b'exit'));assert current['physical']==b'/etc'
        data=send(ch,b"complete -p cd >/dev/null 2>&1; printf 'COMPLETE_WORKS\\n'; history 5");assert b'COMPLETE_WORKS' in data
        ch.send(b'exit\r');receive(ch,False,.5);ch.close();client.close()
        # A user-owned hook replaces integration without being rewritten.
        sf.open(home+'/.bashrc','w').write("case $- in *i*) ;; *) return;; esac\nPS1='CUSTOM_READY> '\nPROMPT_COMMAND=('printf CUSTOM_HOOK')\nPS0='CUSTOM_BUSY'\n")
        client=connect(config,user);ch=client.get_transport().open_session();ch.get_pty();ch.exec_command(bootstrap);data=receive(ch,False,1)
        assert b'CUSTOM_HOOK' in data and b'CUSTOM_READY>' in data and b'SoDeck;1;' not in data
        ch.send(b'false\r');data=receive(ch,False,.5);assert b'CUSTOM_BUSY' in data and b'SoDeck;1;' not in data
        ch.close();client.close();sf.close()
        for hook in ["PS0='CUSTOM_BUSY'", "PROMPT_COMMAND='printf CUSTOM_HOOK'"]:
            sf=root.open_sftp();sf.open(home+'/.bashrc','w').write("case $- in *i*) ;; *) return;; esac\nPS1='CUSTOM_READY> '\n"+hook+'\n');sf.close()
            client=connect(config,user);ch=client.get_transport().open_session();ch.get_pty();ch.exec_command(bootstrap)
            data=receive(ch,False,1);assert b'CUSTOM_READY>' in data and not any(b';ready;' in f for f in re.findall(PATTERN,data))
            ch.send(b'cd /tmp\r');data=receive(ch,False,.5);assert not any(b';ready;' in f for f in re.findall(PATTERN,data))
            ch.close();client.close()
        results.append({'shell':command(root,shell+' --version | head -1').decode().strip(),'result':'login, cd semantics, byte paths, symlink, exit/PIPESTATUS, nested shell, hook override passed'})
    root.close();pathlib.Path(output).write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(results,indent=2))
if __name__=='__main__':
    run(json.loads(pathlib.Path(sys.argv[1]).read_text(encoding='utf-8-sig')),pathlib.Path(sys.argv[2]).read_text(encoding='utf-8-sig'),sys.argv[3])
