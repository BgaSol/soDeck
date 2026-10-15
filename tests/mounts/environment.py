"""Run mount tests using disposable Linux OpenSSH and Rust runner containers.

Build the images documented in tests/rsync/README.md first. On Windows invoke
this script with WSL Python. Only resources created by this run are removed.
"""
import pathlib
import subprocess
import uuid
import argparse

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--mount-count', type=int, default=48)
args = parser.parse_args()
if not 48 <= args.mount_count <= 1024:
    parser.error('--mount-count must be between 48 and 1024')

root = pathlib.Path(__file__).resolve().parents[2]
name = 'sodeck-mount-check-' + uuid.uuid4().hex[:10]


def docker(*args):
    return subprocess.check_output(['docker', *args], text=True).strip()


network = None
volume = None
fixture = None
try:
    network = docker('network', 'create', name)
    volume = docker('volume', 'create', name)
    # Real /proc seq_file reads can return short chunks before EOF. A small
    # mount table misses the libssh2 read-ahead failure seen on Docker hosts.
    many_mounts = []
    for index in range(args.mount_count):
        target = '/mnt/check-many/' + str(index) + '-' + 'm' * 200
        many_mounts.extend(['--tmpfs', target + ':size=1m,mode=0755'])
    fixture = docker('run', '-d', '--network', network, '--network-alias', 'mount-ssh',
        '--tmpfs', '/mnt/mount-check:size=32m,mode=0755',
        '-v', volume + ':/mnt/mount-ro:ro',
        *many_mounts,
        'ssh-workspace-rsync-fixture', 'sh', '-c',
        'ssh-keygen -q -t ed25519 -N "" -f /tmp/mount-host-key && exec /usr/sbin/sshd -D -e -h /tmp/mount-host-key')
    result = subprocess.call(['docker', 'run', '--rm', '--network', network,
        '-e', 'MOUNT_TEST_HOST=mount-ssh', '-e', 'CARGO_BUILD_JOBS=3', '-e', 'CARGO_PROFILE_DEV_DEBUG=0',
        '-e', 'MOUNT_TEST_MOUNT_COUNT=' + str(args.mount_count),
        '-e', 'CARGO_HTTP_MULTIPLEXING=false',
        '-v', str(root) + ':/workspace:ro', '-v', 'ssh-workspace-rsync-target:/cargo-target',
        '-v', 'ssh-workspace-rsync-cargo:/usr/local/cargo/registry',
        'ssh-workspace-rsync-runner', 'bash', '-c',
        'set -euo pipefail; mkdir /tmp/work; tar --exclude=target --exclude=gen -cf - src-tauri dist tests | tar -xf - -C /tmp/work; cd /tmp/work; cargo test --manifest-path src-tauri/Cargo.toml --lib remote_mounts::tests -- --include-ignored --test-threads=1'])
    raise SystemExit(result)
finally:
    if fixture:
        docker('rm', '-f', fixture)
    if volume:
        docker('volume', 'rm', volume)
    if network:
        docker('network', 'rm', network)
