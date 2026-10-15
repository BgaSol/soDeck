"""Create/remove only this test run's isolated OpenSSH A/B containers."""
import json
import pathlib
import subprocess
import sys
import uuid

root = pathlib.Path(__file__).resolve().parents[2]
record = root / '.qa/rsync-environment.json'
def docker(*args):
    return subprocess.check_output(['docker', *args], text=True).strip()
if len(sys.argv) > 1 and sys.argv[1] == 'run':
    saved = json.loads(record.read_text())
    command = sys.argv[2:] or ['cargo', 'test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', 'rsync_', '--', '--ignored', '--test-threads=1']
    raise SystemExit(subprocess.call(['docker', 'run', '--rm', '--network', saved['network'],
        '-e', 'SSH_RSYNC_CONTAINER=1', '-e', 'CARGO_BUILD_JOBS=3', '-e', 'CARGO_PROFILE_DEV_DEBUG=0', '-e', 'CARGO_HTTP_MULTIPLEXING=false',
        '-v', str(root) + ':/workspace', '-v', 'ssh-workspace-rsync-target:/cargo-target',
        '-v', 'ssh-workspace-rsync-cargo:/usr/local/cargo/registry',
        'ssh-workspace-rsync-runner', *command]))
elif len(sys.argv) > 1 and sys.argv[1] == 'stop':
    saved = json.loads(record.read_text())
    for container in saved['containers']:
        docker('rm', '-f', container)
    docker('network', 'rm', saved['network'])
    record.unlink()
else:
    if record.exists():
        raise SystemExit('An environment is already recorded; stop it first.')
    network = 'ssh-workspace-rsync-' + uuid.uuid4().hex[:10]
    docker('network', 'create', network)
    containers = []
    try:
        ports = []
        for side in ['a', 'b']:
            container = docker('run', '-d', '--network', network, '--network-alias', 'rsync-' + side,
                               '--tmpfs', '/home/ops/mounted:uid=1000,gid=1000,mode=0755,size=32m',
                               '-p', '127.0.0.1::22', 'ssh-workspace-rsync-fixture', 'sh', '-c',
                               'ssh-keygen -q -t ed25519 -N "" -f /tmp/test-host-key; exec /usr/sbin/sshd -D -e -h /tmp/test-host-key')
            containers.append(container)
            ports.append(int(docker('port', container, '22/tcp').rsplit(':', 1)[1]))
        record.parent.mkdir(exist_ok=True)
        record.write_text(json.dumps(dict(network=network, containers=containers, portA=ports[0], portB=ports[1])))
        print(record.read_text())
    except BaseException:
        for container in containers:
            docker('rm', '-f', container)
        docker('network', 'rm', network)
        raise
