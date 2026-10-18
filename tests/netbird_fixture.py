"""Stateful NetBird simulation over real loopback SSH; never runs host commands."""
import json
import re
import threading
import time
from pathlib import Path

lock = threading.Lock()
SAMPLE = Path(__file__).resolve().parents[1] / "src-tauri/src/netbird/fixtures/status-connected.json"


def handle(channel, command, root):
    marker = re.search(r"__SODECK_NB_[a-f0-9]+__", command)
    if not marker:
        return False
    command = command.split("; sodeck_nb_code=", 1)[0]
    output, error, code, action = "", "", 0, None
    with lock:
        file = root / "netbird-state.json"
        state = json.loads(file.read_text()) if file.exists() else {"connection": "Connected", "active": "active", "invocation": "one"}
        if command == "uname -s":
            output = "Linux"
        elif command == "command -v netbird":
            if (root / "netbird-missing").exists():
                code = 1
            else:
                output = "/usr/bin/netbird"
        elif "readlink -f" in command:
            output = "/usr/bin/netbird"
        elif "--help" in command:
            output = "Usage: netbird\n --json --daemon-addr --no-browser"
        elif "systemctl show" in command:
            output = f"LoadState=loaded\nActiveState={state['active']}\nExecStart={{ path=/usr/bin/netbird ; argv[]=/usr/bin/netbird service run ; start_time=dynamic-{state['invocation']} ; }}\nEnvironment=\nFragmentPath=/etc/systemd/system/netbird.service\nInvocationID={state['invocation']}\nMainPID=101\n0"
        elif command == "test ! -d /var/run/netbird":
            pass
        elif command == "ss -H -xlpn":
            pid = 202 if (root / 'netbird-other-service').exists() else 101
            output = f'u_str LISTEN 0 4096 /var/run/netbird.sock 1 * 0 users:(("netbird",pid={pid},fd=10))'
        elif command == "timeout --version":
            output = "timeout (GNU coreutils) 9.0"
        elif "status --json" in command:
            if (root / "netbird-slow").exists():
                time.sleep(1)
            if (root / "netbird-permission").exists():
                code, error = 1, "permission denied"
            elif state["active"] != "active":
                code, error = 1, "daemon unavailable"
            else:
                sample = json.loads(SAMPLE.read_text())
                sample["daemonStatus"] = state["connection"]
                output = json.dumps(sample)
        elif "SSH_CONNECTION" in command:
            output = "192.0.2.10 42000 " + ("100.80.0.1" if (root / "netbird-dependent").exists() else "127.0.0.1") + " 22"
        elif command == "ip -j address show":
            output = json.dumps([{"ifname": "mesh42", "addr_info": [{"local": "100.80.0.1"}, {"local": "fd00::1"}]}])
        elif " route get " in command:
            output = json.dumps([{"dev": "eth0", "prefsrc": "127.0.0.1"}])
        elif "command -v timeout" in command:
            output = "/usr/bin/timeout\n/usr/bin/ping"
        elif "ping " in command or "nc " in command:
            output = "one response"
        elif "journalctl" in command:
            output = "2026-10-18T00:00:00Z netbird: connected"
        elif " up --no-browser" in command:
            action, state["connection"] = "up", "Connected"
        elif " down </dev/null" in command:
            action, state["connection"] = "down", "Idle"
        elif " restart netbird.service" in command or " start netbird.service" in command:
            action = "restart" if " restart " in command else "start"
            state["active"], state["invocation"] = "active", state["invocation"] + "-new"
        else:
            code, error = 127, "unhandled NetBird fixture command"
        if action:
            file.write_text(json.dumps(state))
            with (root / "netbird-actions.jsonl").open("a") as events:
                events.write(json.dumps({"action": action}) + "\n")
    try:
        if action and (root / "netbird-drop-receipt").exists():
            channel.close()
            return True
        channel.sendall((output + "\n" + marker.group() + str(code) + "\n").encode())
        if error:
            channel.send_stderr(error.encode())
        channel.send_exit_status(code)
        channel.shutdown_write()
        channel.close()
    except (OSError, EOFError):
        pass
    return True
