"""Run real OpenSSH tests in a disposable network; never expose a host port."""
import pathlib
import subprocess
import uuid

root = pathlib.Path(__file__).resolve().parents[2]
network = "sodeck-forward-" + uuid.uuid4().hex[:10]
containers = []

def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()

docker("network", "create", network)
try:
    for side in ["a", "b"]:
        # MaxSessions=0 denies shell/exec/SFTP while permitting direct-tcpip.
        # Only B can resolve the internal target name to its loopback service.
        command = (
            "ssh-keygen -q -t ed25519 -N '' -f /tmp/forward-host-key && "
            "python3 /fixtures/echo.py & "
            "while test ! -f /tmp/forward-host-key; do sleep 0.1; done; "
            "exec /usr/sbin/sshd -D -e -h /tmp/forward-host-key "
            "-o MaxSessions=0 -o AllowTcpForwarding=local -o PermitOpen="
            + ("forward-b:22" if side == "a" else "echo.internal.invalid:7")
        )
        containers.append(docker("run", "-d", "--network", network,
            "--network-alias", "forward-" + side,
            "--add-host", "echo.internal.invalid:127.0.0.1",
            "-v", str(root / "tests/port-forwarding") + ":/fixtures:ro",
            "ssh-workspace-rsync-fixture", "sh", "-c", command))
    result = subprocess.call(["docker", "run", "--rm", "--network", network,
        "-e", "SSH_FORWARD_OPENSSH=1", "-e", "CARGO_BUILD_JOBS=3", "-e", "CARGO_PROFILE_DEV_DEBUG=0",
        "-v", str(root) + ":/workspace:ro", "-v", "ssh-workspace-rsync-target:/cargo-target",
        "-v", "ssh-workspace-rsync-cargo:/usr/local/cargo/registry",
        "ssh-workspace-rsync-runner", "sh", "-c",
        "mkdir /tmp/work && tar --exclude=target --exclude=gen -cf - src-tauri tests | tar -xf - -C /tmp/work "
        "&& mkdir /tmp/work/dist && cd /tmp/work && "
        "cargo test --manifest-path src-tauri/Cargo.toml --lib forward_openssh_ -- --ignored --nocapture"])
    raise SystemExit(result)
finally:
    for container in containers:
        subprocess.call(["docker", "rm", "-f", container], stdout=subprocess.DEVNULL)
    subprocess.call(["docker", "network", "rm", network], stdout=subprocess.DEVNULL)
