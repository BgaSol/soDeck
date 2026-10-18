"""A dedicated nested Engine; never attach the developer's Docker socket.

Build: docker build -t sodeck-docker-engine-fixture tests/docker-engine
Start: python3 tests/docker-engine/environment.py
Stop:  python3 tests/docker-engine/environment.py stop
"""
import json
from pathlib import Path
import subprocess
import sys
import time
import uuid
record = Path(__file__).resolve().parents[2] / ".qa/docker-engine-environment.json"
def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()
if len(sys.argv) > 1 and sys.argv[1] == "stop":
    if record.exists():
        saved = json.loads(record.read_text())
        if not saved["name"].startswith("sodeck-docker-test-"):
            raise SystemExit("Refusing to remove an unrelated container")
        docker("rm", "-fv", saved["name"])
        record.unlink()
else:
    if record.exists():
        raise SystemExit("Stop the recorded test Engine before creating another")
    name = "sodeck-docker-test-" + uuid.uuid4().hex[:12]
    docker("run", "-d", "--privileged", "--name", name, "--label", "io.sodeck.docker.integration=true", "-p", "127.0.0.1::22", "sodeck-docker-engine-fixture")
    record.parent.mkdir(exist_ok=True)
    record.write_text(json.dumps({"name": name, "port": int(docker("port", name, "22/tcp").rsplit(":", 1)[1])}))
    try:
        for _ in range(90):
            if subprocess.call(["docker", "exec", "-u", "ops", name, "docker", "info"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) == 0:
                print(docker("exec", "-u", "ops", name, "docker", "version"))
                print(docker("exec", "-u", "ops", name, "docker", "compose", "version"))
                break
            time.sleep(1)
        else:
            raise RuntimeError("Nested Engine did not become ready")
    except BaseException:
        docker("rm", "-fv", name)
        record.unlink()
        raise
