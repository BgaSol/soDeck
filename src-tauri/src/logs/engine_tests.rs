//! Runs only against the recorded disposable nested Engine, never the host socket.
use super::*;
use std::path::Path;
fn command(session: &ssh2::Session, command: &str) -> String {
    let r = ssh::exec(session, command).unwrap();
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout.trim().into()
}
fn until(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(45);
    while !predicate() {
        assert!(Instant::now() < end, "log lifecycle did not recover");
        std::thread::sleep(Duration::from_millis(100));
    }
}
struct ContainerCleanup<'a> {
    session: &'a ssh2::Session,
    id: String,
}
impl Drop for ContainerCleanup<'_> {
    fn drop(&mut self) {
        let _ = ssh::exec(
            self.session,
            &format!("docker rm -f {}", ssh::quote(&self.id)),
        );
    }
}
#[test]
#[ignore = "dedicated nested Docker Engine + OpenSSH; tests/docker-engine/environment.py"]
fn docker_engine_log_lifecycle_contract() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let environment: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo.join(".qa/docker-engine-environment.json"))
            .expect("start isolated Engine fixture"),
    )
    .unwrap();
    assert!(environment["name"]
        .as_str()
        .unwrap()
        .starts_with("sodeck-docker-test-"));
    let spec =
        connection::tests::transfer_context("a", environment["port"].as_u64().unwrap() as u16);
    let connection = connection::connect_verified(
        &spec,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::Logs,
    )
    .unwrap();
    let session = connection.session();
    command(session, "docker image pull alpine:3.20");
    for tty in [false, true] {
        let id=command(session,&format!("docker run -d {} alpine:3.20 sh -c {}",if tty{"-t"}else{""},ssh::quote("trap 'exit 0' TERM; echo boot; while :; do if [ -f /tmp/crash ]; then rm /tmp/crash; exit 7; fi; echo tick; echo error >&2; sleep 0.2; done")));
        let _cleanup = ContainerCleanup {
            session,
            id: id.clone(),
        };
        let target = crate::docker::ContainerTarget {
            id,
            name: "log-contract".into(),
            endpoint: "unix:///var/run/docker.sock".into(),
        };
        let source = Source::new(&spec, &target);
        let dir = tempfile::tempdir().unwrap();
        let run = Run::new(source.source_id.clone());
        let output = Channel::new(|_| Ok(()));
        let context = spec.clone();
        let t = target.clone();
        let s = source.clone();
        let root = dir.path().to_path_buf();
        let worker_run = run.clone();
        let worker = std::thread::spawn(move || {
            let result = collect_logs(&context, &t, &s, &root, &worker_run, &output);
            worker_run.finish();
            result
        });
        // Cancellation is guaranteed even if an assertion fails.
        struct Stop(Run);
        impl Drop for Stop {
            fn drop(&mut self) {
                self.0.cancel.cancel();
                let _ = self.0.wait();
            }
        }
        let _stop = Stop(run.clone());
        until(|| log_store::stats(dir.path(), &source.source_id).is_ok_and(|s| s.records >= 5));
        let before = log_store::stats(dir.path(), &source.source_id)
            .unwrap()
            .last_seq;
        command(
            session,
            &format!("docker restart --time 1 {}", ssh::quote(&target.id)),
        );
        until(|| {
            log_store::stats(dir.path(), &source.source_id).is_ok_and(|s| s.last_seq > before + 5)
        });
        command(
            session,
            &format!("docker stop --time 1 {}", ssh::quote(&target.id)),
        );
        until(|| {
            run.observation
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|o| o.state == "exited")
        });
        let stopped = log_store::stats(dir.path(), &source.source_id)
            .unwrap()
            .last_seq;
        command(session, &format!("docker start {}", ssh::quote(&target.id)));
        until(|| {
            log_store::stats(dir.path(), &source.source_id).is_ok_and(|s| s.last_seq > stopped + 5)
        });
        command(
            session,
            &format!(
                "docker update --restart=on-failure {}",
                ssh::quote(&target.id)
            ),
        );
        let before_crash = log_store::stats(dir.path(), &source.source_id)
            .unwrap()
            .last_seq;
        command(
            session,
            &format!("docker exec {} touch /tmp/crash", ssh::quote(&target.id)),
        );
        until(|| {
            log_store::stats(dir.path(), &source.source_id)
                .is_ok_and(|s| s.last_seq > before_crash + 5)
        });
        run.cancel.cancel();
        run.wait().unwrap();
        worker.join().unwrap().unwrap();
        let snapshot = snapshot::Snapshot::create(
            dir.path(),
            &source.source_id,
            &dir.path().join("cache"),
            &Cancellation::default(),
        )
        .unwrap();
        let rows = snapshot
            .query(&log_store::Query {
                keyword: "boot".into(),
                limit: 100,
                ..Default::default()
            })
            .unwrap()
            .rows;
        assert_eq!(
            rows.len(),
            4,
            "one original boot record per actual start, no replay duplicates"
        );
        assert!(!lifecycle::timeline(dir.path(), &source.source_id, 0, None)
            .unwrap()
            .boots
            .is_empty());
    }
    let label="--label com.docker.compose.project=sodeck-log-contract --label com.docker.compose.service=api --label com.docker.compose.container-number=1";
    let old = command(
        session,
        &format!("docker run -d {label} alpine:3.20 sleep 120"),
    );
    let old_cleanup = ContainerCleanup {
        session,
        id: old.clone(),
    };
    let target = crate::docker::ContainerTarget {
        id: old.clone(),
        name: "replacement-old".into(),
        endpoint: "unix:///var/run/docker.sock".into(),
    };
    let source = Source::new(&spec, &target);
    let root = tempfile::tempdir().unwrap();
    read_logs(
        &spec,
        &target,
        &source,
        root.path(),
        true,
        &Run::new(source.source_id.clone()),
        &Channel::new(|_| Ok(())),
    )
    .unwrap();
    let daemon: String =
        serde_json::from_str(&command(session, "docker info --format '{{json .ID}}'")).unwrap();
    lifecycle::set_metadata(root.path(), &source.source_id, "daemonId", &daemon).unwrap();
    drop(old_cleanup);
    let new = command(
        session,
        &format!("docker run -d {label} alpine:3.20 sleep 120"),
    );
    let _new_cleanup = ContainerCleanup {
        session,
        id: new.clone(),
    };
    let found = replacements::discover(&spec, &target, root.path(), &source).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, new);
    let missing = read_logs(
        &spec,
        &target,
        &source,
        root.path(),
        false,
        &Run::new(source.source_id.clone()),
        &Channel::new(|_| Ok(())),
    )
    .unwrap_err();
    assert_eq!(protocol::classify(&missing), protocol::Failure::Missing);
    let unsupported = command(
        session,
        "docker run -d --log-driver none alpine:3.20 sleep 120",
    );
    let _unsupported_cleanup = ContainerCleanup {
        session,
        id: unsupported.clone(),
    };
    let target = crate::docker::ContainerTarget {
        id: unsupported,
        name: "unsupported".into(),
        endpoint: "unix:///var/run/docker.sock".into(),
    };
    let source = Source::new(&spec, &target);
    let error = read_logs(
        &spec,
        &target,
        &source,
        root.path(),
        true,
        &Run::new(source.source_id.clone()),
        &Channel::new(|_| Ok(())),
    )
    .unwrap_err();
    assert_eq!(protocol::classify(&error), protocol::Failure::Driver);
    assert_eq!(
        log_store::stats(root.path(), &source.source_id)
            .unwrap()
            .records,
        0
    );
}
