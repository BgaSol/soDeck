"""Stateful Docker simulation for SSH protocol tests; never executes host commands."""
import json
import hashlib
import re
import shlex
import threading
import time

lock = threading.Lock()


def handle(channel, command, root):
    database = root / "docker-management.json"
    if not database.exists():
        return False
    if not any(part in command for part in (" container ", " ps -a ", " stats ", " top ", " image ")):
        return False
    if "__SODECK_BINARY_RESULT_" in command:
        return binary(channel, command, root)
    code, output, error = 0, "", ""
    marker = re.search(r"__SODECK_DOCKER_RESULT_[a-f0-9]+__", command)
    mode_file = root / "docker-operation-mode"
    mode = mode_file.read_text().strip() if mode_file.exists() else ""
    action = ""
    args = shlex.split(command.split("; sodeck_docker_status=", 1)[0])
    cleanup_path = None
    if args[:2] == ["sh", "-c"]:
        inner = args[2]
        args = shlex.split(inner)
        # The producer uses a fixed trap followed by one Docker command.
        cleanup = shlex.split(args[1])
        if "--" in cleanup:
            cleanup_path = root / "remote" / cleanup[cleanup.index("--") + 1].lstrip("/")
        args = args[args.index("EXIT;") + 1:]
    with lock:
        values = json.loads(database.read_text())
        if " image " in command:
            image_file = root / "docker-images.json"
            images = json.loads(image_file.read_text()) if image_file.exists() else {}
            index = args.index("image")
            action = args[index + 1]
            def resolve(ref):
                return next(((iid, image) for iid, image in images.items() if iid == ref or ref in image.get("RepoTags", [])), (None, None))
            iid, image = resolve(args[-2] if action == "tag" else args[-1])
            if action == "pull":
                iid = "sha256:" + "d" * 64
                image = {"Id": iid, "RepoTags": [args[-1]], "RepoDigests": [], "Created": "2026-10-17T00:00:00Z", "Os": "linux", "Architecture": "amd64", "Size": 12345}
                images[iid] = image
                output = "Pull complete\nDigest: " + iid
            elif not image:
                code, error = 1, "No such image: " + args[-1]
            elif action == "inspect":
                output = json.dumps(image)
            elif action == "tag":
                ref = args[-1]
                for value in images.values():
                    value["RepoTags"] = [tag for tag in value.get("RepoTags", []) if tag != ref]
                image["RepoTags"].append(ref)
            elif action == "rm":
                if args[-1] == iid:
                    if any(value.get("imageId") == iid for value in values.values()) or len(image.get("RepoTags", [])) > 1:
                        code, error = 1, "conflict: image is in use"
                    else:
                        del images[iid]
                else:
                    image["RepoTags"].remove(args[-1])
                output = "Untagged: " + args[-1]
            else:
                return False
            if action != "inspect":
                image_file.write_text(json.dumps(images))
        elif " stats " in command:
            output = json.dumps({"ID": args[-1], "CPUPerc": "145.3%", "MemUsage": "12MiB / 1GiB", "MemPerc": "1.2%", "NetIO": "1MB / 2MB", "BlockIO": "0B / 2MB", "PIDs": "4"})
        elif " top " in command:
            output = "UID PID CMD\nroot 1 nginx"
        elif " container ls " in command:
            requested = next((arg.split("=", 2)[-1] for arg in args if arg.startswith("label=io.sodeck.create.request=")), None)
            output = "\n".join(cid for cid, value in values.items() if requested is None or value.get("requestId") == requested)
        elif " ps -a " in command:
            if "--filter health=unhealthy" in command:
                output = "\n".join(cid for cid, value in values.items() if value.get("health") == "unhealthy")
            else:
                output = "\n".join(json.dumps({"ID": cid, "Names": value["name"], "Image": "app:latest", "State": value["state"], "Status": value["state"]}) for cid, value in values.items())
        else:
            index = args.index("container")
            action = args[index + 1]
            cid = args[index + 2] if action == "rename" else args[-1]
            value = values.get(cid)
            if action == "create":
                name = args[args.index("--name") + 1]
                cid = hashlib.sha256(name.encode()).hexdigest()
                if any(value["name"] == name for value in values.values()):
                    code, error = 1, "name already in use"
                else:
                    request = args[args.index("--label") + 1].split("=", 1)[1]
                    values[cid] = {"name": name, "state": "created", "requestId": request, "imageId": args[args.index("--env-file") + 2] if "--env-file" in args else "sha256:" + "c" * 64}
                    if "--env-file" in args:
                        path = root / "remote" / args[args.index("--env-file") + 1].lstrip("/")
                        (root / "create-env-receipt").write_text(path.read_text())
                        (root / "create-env-mode").write_text(str(path.stat().st_mode & 0o777))
                    database.write_text(json.dumps(values))
                    output = cid
            if action == "inspect" and '"Image":{{json .Image}}' in command:
                first_id = args.index("--format") + 2
                output = "\n".join(json.dumps({"Id": key, "Image": values[key].get("imageId", "sha256:" + "c" * 64), "Name": "/" + values[key]["name"], "State": values[key]["state"]}) for key in args[first_id:] if key in values)
                value = None
                code = 0
            elif action == "inspect" and "--format" not in args and value:
                output = json.dumps([{"Id": cid, "Config": {"Image": "app:latest", "Env": ["PASSWORD=fixture-secret"], "Labels": {"token": "fixture-secret"}, "Cmd": ["run", "fixture-secret"]}}])
                value = None
                code = 0
            if output:
                pass
            elif action == "create":
                pass
            elif value is None:
                code, error = 1, "Error: No such container: " + cid
            elif action == "inspect":
                if mode == "verify-fail" and value.get("mutated"):
                    code, error = 1, "fixture inspect temporarily unavailable"
                else:
                    output = json.dumps({
                        "Id": cid, "Name": "/" + value["name"], "Image": "app:latest",
                        "State": {"Status": value["state"], "ExitCode": value.get("exit", 0), "OOMKilled": value.get("oom", False), "Error": "", "StartedAt": "2026-10-17T01:00:00Z", "FinishedAt": "0001-01-01T00:00:00Z", "Health": {"Status": value["health"], "FailingStreak": 0} if value.get("health") else None},
                        "Created": "2026-10-01T01:00:00Z", "RestartCount": value.get("restarts", 0), "RestartPolicy": {"Name": value.get("policy", "unless-stopped"), "MaximumRetryCount": value.get("maximum", 0)},
                        "AutoRemove": value.get("autoRemove", False), "StopTimeout": 10,
                        "Ports": {"80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}], "53/udp": None},
                        "Networks": {"bridge": {"NetworkID": "n" * 64, "IPAddress": "172.17.0.2", "GlobalIPv6Address": ""}},
                        "Mounts": [{"Type": "bind", "Source": "/data/app", "Destination": "/app/data", "RW": True}],
                    })
            elif action in ("start", "stop", "restart", "rm", "pause", "unpause", "rename", "update", "kill"):
                if mode == "fail":
                    code, error = 1, "fixture operation rejected"
                elif action == "rm" and value["state"] == "running":
                    code, error = 1, "container is running"
                else:
                    value["mutated"] = True
                    if action == "rename":
                        value["name"] = args[-1]
                    elif action == "update":
                        policy = args[args.index("--restart") + 1].split(":")
                        value["policy"], value["maximum"] = policy[0], int(policy[1]) if len(policy) > 1 else 0
                    elif action == "pause":
                        value["state"] = "paused"
                    elif action == "rm" or (value.get("autoRemove") and action in ("stop", "restart")):
                        del values[cid]
                    else:
                        value["state"] = "exited" if action in ("stop", "kill") else "running"
                        if action == "restart":
                            value["restarts"] = value.get("restarts", 0) + 1
                    database.write_text(json.dumps(values))
                    output = cid
            else:
                return False
    if action in ("start", "stop", "restart", "rm", "pause", "unpause", "rename", "update", "kill"):
        delay = root / "docker-action-delay"
        if delay.exists():
            time.sleep(float(delay.read_text()))
        if mode == "drop":
            channel.get_transport().close()
            return True
    if cleanup_path:
        cleanup_path.unlink(missing_ok=True)
    try:
        if output:
            channel.sendall((output + "\n").encode())
        if error:
            channel.sendall_stderr(error.encode())
        if marker and mode != "no-receipt":
            channel.sendall(("\n" + marker.group() + str(code) + "\n").encode())
        channel.send_exit_status(code)
        channel.shutdown_write()
        channel.close()
    except (OSError, EOFError):
        pass
    return True


def binary(channel, command, root):
    marker = re.search(r"__SODECK_BINARY_RESULT_[a-f0-9]+__", command).group()
    args = shlex.split(command.split("; sodeck_status=", 1)[0])
    upload = "load" in args or ("cp" in args and args[args.index("cp") + 1] == "-")
    mode = (root / "docker-binary-mode").read_text().strip() if (root / "docker-binary-mode").exists() else ""
    try:
        if upload:
            value = bytearray()
            while True:
                chunk = channel.recv(65536)
                if not chunk:
                    break
                value.extend(chunk)
            (root / "docker-binary-upload").write_bytes(value)
            channel.sendall(b"Loaded fixture\n")
        else:
            value = bytes(range(256)) * 8192
            # Make stderr larger than the SSH window in multiple chunks, while
            # alternating binary stdout; neither pipe may block the other.
            for start in range(0, len(value), 8192):
                channel.sendall(value[start:start + 8192])
                channel.sendall_stderr(b"fixture diagnostic\n" * 30)
                if mode == "slow":
                    time.sleep(0.02)
        if mode != "no-receipt":
            channel.sendall_stderr(("\n" + marker + "0\n").encode())
        channel.send_exit_status(0)
        channel.shutdown_write()
        channel.close()
    except (OSError, EOFError):
        pass
    return True
