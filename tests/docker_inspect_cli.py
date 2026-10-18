"""Exercise production inspect templates with a real Docker CLI, without a daemon.

The Rust test passes the exact templates as arguments. Only Engine HTTP responses
are fixtures; template evaluation (including typed/raw fallback) belongs to Docker.
"""

import copy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import re
import subprocess
import sys
import tempfile
import threading


IMAGE_ID = "sha256:" + "b" * 64
CONTAINER_ID = "a" * 64
CANARY = "inspect-private-canary"
IMAGE = {
    "Id": IMAGE_ID,
    "RepoTags": ["example/app:latest"],
    "RepoDigests": [],
    "Created": "2026-10-18T00:00:00Z",
    "Os": "linux",
    "Architecture": "amd64",
    "Size": 9007199254740993,
    "Config": {"Env": ["SECRET=" + CANARY]},
}
CONTAINER = {
    "Id": CONTAINER_ID,
    "Name": "/example",
    "Image": IMAGE_ID,
    "Config": {"Image": "example/app:latest", "Env": ["SECRET=" + CANARY]},
    "State": {
        "Status": "running",
        "ExitCode": 0,
        "OOMKilled": False,
        "Error": "",
        "StartedAt": "2026-10-18T00:00:00Z",
        "FinishedAt": "0001-01-01T00:00:00Z",
    },
    "Created": "2026-10-18T00:00:00Z",
    "RestartCount": 0,
    "HostConfig": {
        "RestartPolicy": {"Name": "no", "MaximumRetryCount": 0},
        "AutoRemove": False,
    },
    "NetworkSettings": {"Ports": {"80/tcp": None}, "Networks": {}},
    "Mounts": [],
}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        path = re.sub(r"^/v[0-9.]+", "", self.path)
        if path == "/_ping":
            body = b"OK"
        elif path == "/images/" + IMAGE_ID + "/json":
            body = json.dumps(self.server.image).encode()
        elif path == "/containers/" + CONTAINER_ID + "/json":
            body = json.dumps(self.server.container).encode()
        else:
            self.send_error(404, "Unexpected fixture request")
            return
        self.send_response(200)
        self.send_header("API-Version", "1.47")
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    do_HEAD = do_GET

    def log_message(self, *_):
        pass


def exercise(container_format, image_format):
    cli = os.environ.get("DOCKER_TEST_CLI", "docker")
    # Use only this loopback fixture, independent of the developer's contexts/TLS.
    env = {key: value for key, value in os.environ.items() if not key.startswith("DOCKER_")}
    env.update(DOCKER_API_VERSION="1.47", NO_PROXY="127.0.0.1", no_proxy="127.0.0.1")
    results = {"images": [], "containers": []}
    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server, tempfile.TemporaryDirectory() as config:
        server.image = IMAGE
        server.container = CONTAINER
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        prefix = [cli, "--config", config, "--host", f"tcp://127.0.0.1:{server.server_port}"]

        def inspect(kind, template, target, case, error_key=None):
            result = subprocess.run(
                prefix + [kind, "inspect", "--format", template, target],
                env=env, capture_output=True, text=True, timeout=15,
            )
            if error_key is not None:
                assert result.returncode != 0 and error_key in result.stderr, (case, result)
                return
            assert result.returncode == 0, f"{case}: {result.stderr}"
            assert CANARY not in result.stdout, f"{case}: private data escaped projection"
            return json.loads(result.stdout)

        try:
            for case, variant in [("absent", None), ("null", None), ("empty", ""), ("arm", "v8")]:
                server.image = copy.deepcopy(IMAGE)
                if case != "absent":
                    server.image["Variant"] = variant
                if case == "arm":
                    server.image["Architecture"] = "arm64"
                value = inspect("image", image_format, IMAGE_ID, "image-" + case)
                expected = {key: field for key, field in server.image.items() if key != "Config"}
                expected["Variant"] = variant
                assert value == expected, (case, value)
                results["images"].append(value)

            cases = [("absent", None, None), ("null", None, None),
                     ("starting", "starting", 0), ("healthy", "healthy", -1),
                     ("unhealthy", "unhealthy", 10)]
            for case, health, timeout in cases:
                server.container = copy.deepcopy(CONTAINER)
                if case != "absent":
                    server.container["State"]["Health"] = None if health is None else {
                        "Status": health,
                        "FailingStreak": 3 if health == "unhealthy" else 0,
                        "Log": [{"Output": CANARY}],
                    }
                    server.container["Config"]["StopTimeout"] = timeout
                value = inspect("container", container_format, CONTAINER_ID, "container-" + case)
                projected_health = value["State"]["Health"]
                expected_health = None if health is None else {
                    "Status": health, "FailingStreak": 3 if health == "unhealthy" else 0,
                }
                assert projected_health == expected_health, (case, value)
                assert value["StopTimeout"] == timeout, (case, value)
                assert value["Id"] == CONTAINER_ID and value["State"]["Status"] == "running"
                assert value["Image"] == "example/app:latest"
                assert value["Ports"] == {"80/tcp": None}
                results["containers"].append(value)

            # Required identity/state fields must still fail when absent.
            del server.image["Id"]
            inspect("image", image_format, IMAGE_ID, "missing-image-id", "Id")
            del server.container["State"]["Status"]
            inspect("container", container_format, CONTAINER_ID, "missing-state", "Status")
        finally:
            server.shutdown()
            worker.join(timeout=5)
    return results


if __name__ == "__main__":
    print(json.dumps(exercise(*sys.argv[1:])))
