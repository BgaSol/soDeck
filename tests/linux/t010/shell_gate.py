"""DR-07: compare an ordinary SSH shell with the proposed in-memory Bash startup.

Only the isolated T-010 guest is accepted. The dedicated account and evidence
remain for inspection; the test never edits tester's or a real user's dotfiles.
"""
import argparse
import json
import pathlib
import shlex
import time

import paramiko


def connect(config, user):
    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(config['host'], port=config['port'], username=user,
                   key_filename=config['privateKeyPath'], timeout=10)
    return client


def command(client, script):
    _, stdout, stderr = client.exec_command(script, timeout=20)
    out, error = stdout.read(), stderr.read()
    if stdout.channel.recv_exit_status():
        raise RuntimeError(error.decode(errors='replace'))
    return out


def drain(channel, marker, timeout=15):
    data = b''
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if channel.recv_ready():
            data += channel.recv(65536)
            if marker in data:
                return data
        elif channel.closed:
            break
        time.sleep(.025)
    raise RuntimeError('Expected terminal marker not received: ' + repr(data[-2000:]))


def run(config, output):
    assert config['host'] == '127.0.0.1' and config['port'] == 22281
    root = connect(config, 'root')
    account = 'sodeck-t010-gate'
    home = '/home/' + account
    setup = f'''set -eu
id {account} >/dev/null 2>&1 || useradd -m -s /bin/bash {account}
install -d -o {account} -g {account} -m 700 {home}/.ssh
install -o {account} -g {account} -m 600 /root/.ssh/authorized_keys {home}/.ssh/authorized_keys
'''
    command(root, setup)
    sftp = root.open_sftp()
    fixtures = {
        '.bash_profile': "printf 'profile:%s\\n' \"$-\" >> ~/startup.log\n. ~/.bashrc\n",
        '.bashrc': "printf 'rc:%s\\n' \"$-\" >> ~/startup.log\ncase $- in *i*) ;; *) return;; esac\nPS1='GATE_READY> '\n",
    }
    for name, text in fixtures.items():
        with sftp.open(home + '/' + name, 'w') as f:
            f.write(text)
    command(root, f'chown {account}:{account} {home}/.bash_profile {home}/.bashrc')
    # This is the accepted candidate, not a fallback which edits login files.
    bootstrap = "__sodeck_report() { local rc=$?; printf '\\033]633;SoDeckGate;%s\\033\\\\' \"$PWD\"; return \"$rc\"; }; export -f __sodeck_report; export PROMPT_COMMAND=__sodeck_report; exec /bin/bash --login -i"
    results = {}
    for mode in ['shell', 'exec-environment']:
        command(root, f': > {home}/startup.log; chown {account}:{account} {home}/startup.log')
        client = connect(config, account)
        channel = client.get_transport().open_session()
        channel.get_pty(term='xterm-256color', width=100, height=30)
        if mode == 'shell':
            channel.invoke_shell()
        else:
            channel.exec_command(bootstrap)
        first = drain(channel, b'GATE_READY> ')
        # Commands below are test input, never product initialization injection.
        channel.send(b"printf 'LOGIN=%s\\n' \"$(shopt -q login_shell && echo yes || echo no)\"; false\n")
        second = drain(channel, b'GATE_READY> ')
        channel.send(b"printf 'STATUS=%s\\n' \"$?\"\n")
        third = drain(channel, b'GATE_READY> ')
        channel.send(b'exit\n')
        channel.close()
        client.close()
        with sftp.open(home + '/startup.log', 'r') as f:
            startup = f.read().decode().splitlines()
        results[mode] = {'startup': startup,
                         'reportSeen': b'SoDeckGate;' in first,
                         'loginShell': b'LOGIN=yes' in second,
                         'exitStatusPreserved': b'STATUS=1' in third,
                         'transcript': (first + second + third).decode(errors='replace')}
    results['bashVersion'] = command(root, '/bin/bash --version | head -n1').decode().strip()
    results['startupEquivalent'] = results['shell']['startup'] == results['exec-environment']['startup']
    results['adoptable'] = results['startupEquivalent'] and results['exec-environment']['reportSeen'] and results['exec-environment']['exitStatusPreserved']
    sftp.close()
    root.close()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(results, indent=2), encoding='utf-8')
    print(json.dumps({k: v for k, v in results.items() if k not in ['shell', 'exec-environment']}))
    print(json.dumps({mode: results[mode]['startup'] for mode in ['shell', 'exec-environment']}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    parser.add_argument('--output', required=True, type=pathlib.Path)
    args = parser.parse_args()
    run(json.loads(pathlib.Path(args.config).read_text(encoding='utf-8-sig')), args.output)
