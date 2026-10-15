# Real rsync integration tests

Run from the repository root. These fixtures create their own Docker network and two Ubuntu OpenSSH servers. Password `fixture-password` is test-only. No saved application configuration or business server is used.

```sh
docker build -t ssh-workspace-rsync-fixture tests/rsync
python3 tests/rsync/environment.py
cargo test --manifest-path src-tauri/Cargo.toml --lib rsync_ -- --ignored --test-threads=1
python3 tests/rsync/environment.py stop
```

When a native test executable cannot run, build the Linux runner (requires a Linux Docker engine):

```sh
docker build -t ssh-workspace-rsync-runner -f tests/rsync/Runner.Dockerfile tests/rsync
python3 tests/rsync/environment.py run
# All unit tests, using the same cached Linux build:
python3 tests/rsync/environment.py run cargo test --manifest-path src-tauri/Cargo.toml --lib
```

On Windows these Python/Docker commands can run through WSL. `environment.py stop` removes only the containers/network recorded in `.qa/rsync-environment.json`. Cargo cache volumes remain for subsequent builds.

Tests call the production Rust pipeline, SSH transport, SFTP commit logic, rsync command builder and recovery code. A → B uses the internal hostname `rsync-b:22`; management connections use different published addresses. Fault injection in the test image wraps real rsync with a rate limit or deliberate nonzero exit and a large diagnostic stream. Run serially because fault modes are per test server.
