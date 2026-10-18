//! Runs only against the independently provisioned nested test Engine. No host
//! socket is mounted; cleanup removes only this test run's enclosing container.
use super::*;
use crate::{connection, ssh_transport::Cancellation, traffic::TrafficPurpose};
use model::*;
use std::{
    io::Write,
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn run(session: &ssh2::Session, command: &str) -> crate::ssh::ExecResult {
    let result = executor::execute_progress(
        session,
        command,
        Duration::from_secs(600),
        &AtomicBool::new(false),
        &|| Ok(()),
        &|_| {},
    )
    .unwrap();
    assert_eq!(result.code, 0, "{}", result.stderr);
    result
}
#[test]
#[ignore = "dedicated nested Docker Engine + OpenSSH; tests/docker-engine/environment.py"]
fn docker_engine_management_contract() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let environment: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo.join(".qa/docker-engine-environment.json"))
            .expect("start isolated Docker Engine fixture"),
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
        TrafficPurpose::DockerAction,
    )
    .unwrap();
    let session = connection.session();
    let status = query::detect_on(session, &|| Ok(())).unwrap();
    assert_eq!(status.state, "running");
    let engine = EngineBinding {
        token: "integration/1".into(),
        generation: "integration".into(),
        engine_generation: 1,
        endpoint: status.endpoint,
        daemon_id: status.daemon_id,
    };
    let prefix = query::docker(&engine.endpoint);
    let check = || Ok(());
    run(session, &format!("{prefix} image pull alpine:3.20"));
    let image = images::resolve_local(session, &engine.endpoint, "alpine:3.20", &check)
        .unwrap()
        .unwrap();
    let name = format!("sodeck-{}", uuid::Uuid::new_v4().simple());
    let mut input = create::Spec {
        image_id: image.clone(),
        name: name.clone(),
        ports: vec![],
        environment: vec![create::Environment {
            key: "SODECK_CANARY".into(),
            value: "not-in-task-history;$'\"".into(),
        }],
        mounts: vec![],
        network: None,
        restart_policy: "no".into(),
        maximum_retry_count: 0,
        command: vec![
            "/bin/sh".into(),
            "-c".into(),
            "trap 'sleep 7; exit 0' TERM; while :; do sleep 0.2; done".into(),
        ],
        working_directory: String::new(),
        start: true,
    };
    create::prepare(session, &engine, &input, &check).unwrap();
    let created = std::sync::Mutex::new(String::new());
    let request = uuid::Uuid::new_v4().to_string();
    let result = create::execute(
        session,
        &engine.endpoint,
        &input,
        &request,
        &AtomicBool::new(false),
        &check,
        &|id| *created.lock().unwrap() = id.into(),
    )
    .unwrap();
    assert_eq!(result.code, 0, "{}", result.stderr);
    let id = created.into_inner().unwrap();
    let actual_env: Vec<String> = serde_json::from_str(
        &run(
            session,
            &format!("{prefix} container inspect --format '{{{{json .Config.Env}}}}' {id}"),
        )
        .stdout,
    )
    .unwrap();
    assert!(actual_env.contains(&format!("SODECK_CANARY={}", input.environment[0].value)));
    assert_eq!(
        inspect::container(session, &engine.endpoint, &id, &check)
            .unwrap()
            .state,
        "running"
    );
    assert!(create::observe(session, &engine.endpoint, &request, &check)
        .unwrap()
        .0
        .contains(&id));
    assert!(stats::sample(session, &engine.endpoint, &id, &check)
        .unwrap()
        .cpu_percent
        .is_some());
    for action in [
        Action::Pause,
        Action::Unpause,
        Action::RestartPolicy {
            policy: "on-failure".into(),
            maximum_retry_count: 3,
        },
        Action::Rename {
            name: format!("{name}-renamed"),
        },
    ] {
        run(session, &action.command(&engine.endpoint, &id).unwrap());
    }
    run(
        session,
        &Action::RestartPolicy {
            policy: "no".into(),
            maximum_retry_count: 0,
        }
        .command(&engine.endpoint, &id)
        .unwrap(),
    );
    std::thread::sleep(Duration::from_secs(1));
    let started = Instant::now();
    run(
        session,
        &Action::Stop.command(&engine.endpoint, &id).unwrap(),
    );
    assert!(started.elapsed() > Duration::from_secs(6));
    let detail = images::detail(session, &engine.endpoint, &image, &check).unwrap();
    assert!(detail
        .containers
        .iter()
        .any(|c| c.id == id && c.state == "exited"));
    let tag = format!("{name}:copy");
    let tag_action = Action::ImageTag {
        reference: tag.clone(),
    };
    images::prepare(session, &engine.endpoint, &image, &tag_action, &check).unwrap();
    run(
        session,
        &tag_action.command(&engine.endpoint, &image).unwrap(),
    );
    let remove_tag = Action::ImageRemove {
        reference: Some(tag.clone()),
    };
    images::prepare(session, &engine.endpoint, &image, &remove_tag, &check).unwrap();
    run(
        session,
        &remove_tag.command(&engine.endpoint, &image).unwrap(),
    );
    assert!(
        images::resolve_local(session, &engine.endpoint, &tag, &check)
            .unwrap()
            .is_none()
    );
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("image.tar");
    let save = transfer::selection(
        transfer::Kind::ImageSave,
        image.clone(),
        String::new(),
        archive.clone(),
        "rename".into(),
    )
    .unwrap();
    transfer::prepare(session, &engine, &save, &check).unwrap();
    assert_eq!(
        transfer::execute(
            session,
            &engine.endpoint,
            &save,
            &AtomicBool::new(false),
            &check,
            &|_, _| {}
        )
        .unwrap()
        .code,
        0
    );
    assert!(archive.metadata().unwrap().len() > 1000);
    let load = transfer::selection(
        transfer::Kind::ImageLoad,
        "image-import".into(),
        String::new(),
        archive,
        "overwrite".into(),
    )
    .unwrap();
    assert_eq!(
        transfer::execute(
            session,
            &engine.endpoint,
            &load,
            &AtomicBool::new(false),
            &check,
            &|_, _| {}
        )
        .unwrap()
        .code,
        0
    );
    let source = temp.path().join("binary.txt");
    let bytes: Vec<u8> = (0..=255).cycle().take(200000).collect();
    std::fs::write(&source, &bytes).unwrap();
    let copy = transfer::selection(
        transfer::Kind::CopyIn,
        id.clone(),
        "/tmp".into(),
        source,
        "overwrite".into(),
    )
    .unwrap();
    assert_eq!(
        transfer::execute(
            session,
            &engine.endpoint,
            &copy,
            &AtomicBool::new(false),
            &check,
            &|_, _| {}
        )
        .unwrap()
        .code,
        0
    );
    let out = transfer::selection(
        transfer::Kind::CopyOut,
        id.clone(),
        "/tmp/binary.txt".into(),
        temp.path().join("container.tar"),
        "rename".into(),
    )
    .unwrap();
    assert_eq!(
        transfer::execute(
            session,
            &engine.endpoint,
            &out,
            &AtomicBool::new(false),
            &check,
            &|_, _| {}
        )
        .unwrap()
        .code,
        0
    );
    assert!(std::fs::read(&out.local)
        .unwrap()
        .windows(bytes.len())
        .any(|v| v == bytes));
    for action in [Action::VolumeCreate, Action::NetworkCreate] {
        let plan = resources::prepare(session, &engine.endpoint, &name, &action, &check).unwrap();
        operations::execute(
            session,
            plan,
            &AtomicBool::new(false),
            &check,
            &|_| {},
            &|_, _| {},
        )
        .unwrap();
    }
    let all = resources::overview(session, &engine.endpoint, &check).unwrap();
    let network = all.networks.iter().find(|n| n.name == name).unwrap();
    for action in [
        Action::NetworkConnect {
            container_id: id.clone(),
        },
        Action::NetworkDisconnect {
            container_id: id.clone(),
        },
        Action::NetworkRemove,
    ] {
        let plan =
            resources::prepare(session, &engine.endpoint, &network.id, &action, &check).unwrap();
        assert_eq!(
            operations::execute(
                session,
                plan,
                &AtomicBool::new(false),
                &check,
                &|_| {},
                &|_, _| {}
            )
            .unwrap()
            .code,
            0
        );
    }
    let plan = resources::prepare(
        session,
        &engine.endpoint,
        &name,
        &Action::VolumeRemove,
        &check,
    )
    .unwrap();
    assert_eq!(
        operations::execute(
            session,
            plan,
            &AtomicBool::new(false),
            &check,
            &|_| {},
            &|_, _| {}
        )
        .unwrap()
        .code,
        0
    );
    // Frozen Compose configurations include secrets only in a private remote file.
    let state = connection::tests::state();
    let path = format!("/tmp/{name}.json");
    let sftp = session.sftp().unwrap();
    let compose_volume = format!("{name}-compose-data");
    let configuration = serde_json::json!({"services":{"app":{"image":"alpine:3.20","command":["sleep","300"],"environment":{"SECRET":"compose-canary-$$HOME"},"volumes":["data:/data"]}},"volumes":{"data":{"name":compose_volume}}});
    sftp.create(Path::new(&path))
        .unwrap()
        .write_all(configuration.to_string().as_bytes())
        .unwrap();
    let association = state
        .docker
        .with_store(|s| {
            s.save(compose::Association {
                id: String::new(),
                revision: 0,
                server_id: spec.target().id.clone(),
                endpoint: engine.endpoint.clone(),
                daemon_id: engine.daemon_id.clone(),
                project: name.clone(),
                directory: "/tmp".into(),
                files: vec![path.clone()],
                env_files: vec![],
                profiles: vec![],
            })
        })
        .unwrap();
    for mode in [
        compose::Mode::Apply,
        compose::Mode::Stop,
        compose::Mode::Start,
        compose::Mode::Restart,
        compose::Mode::Down,
    ] {
        let plan = compose::prepare(
            &state,
            &spec,
            session,
            &engine,
            &name,
            &mode,
            &association.id,
            association.revision,
            &check,
        )
        .unwrap();
        assert!(!serde_json::to_string(&plan.consequences)
            .unwrap()
            .contains("compose-canary"));
        let result = operations::execute(
            session,
            plan,
            &AtomicBool::new(false),
            &check,
            &|_| {},
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(result.code, 0, "{}", result.stderr);
        if mode == compose::Mode::Apply {
            let projects = compose::projects(session, &engine.endpoint, &check).unwrap();
            let service = &projects.iter().find(|p| p.name == name).unwrap().containers[0];
            let env: Vec<String> = serde_json::from_str(
                &run(
                    session,
                    &format!(
                        "{prefix} container inspect --format '{{{{json .Config.Env}}}}' {}",
                        service.id
                    ),
                )
                .stdout,
            )
            .unwrap();
            assert!(env.contains(&"SECRET=compose-canary-$HOME".to_string()));
            assert!(resources::prepare(
                session,
                &engine.endpoint,
                &compose_volume,
                &Action::VolumeRemove,
                &check
            )
            .is_err());
        }
    }
    let retained =
        resources::detail(session, &engine.endpoint, "volume", &compose_volume, &check).unwrap();
    assert!(
        retained.containers.is_empty(),
        "Down retains the named volume"
    );
    run(session, &format!("{prefix} volume rm {compose_volume}"));
    let automatic = run(
        session,
        &format!("{prefix} run -d --rm {} sleep 300", image),
    )
    .stdout
    .trim()
    .to_string();
    assert!(
        inspect::container(session, &engine.endpoint, &automatic, &check)
            .unwrap()
            .auto_remove
    );
    run(
        session,
        &Action::Stop.command(&engine.endpoint, &automatic).unwrap(),
    );
    assert_eq!(
        inspect::container(session, &engine.endpoint, &automatic, &check)
            .unwrap_err()
            .kind,
        "notFound"
    );
    let entries = vec![batch::Entry {
        target_id: id,
        action: batch::Kind::RemoveContainer,
    }];
    let plan = batch::prepare(session, &engine.endpoint, &entries, &check).unwrap();
    assert_eq!(
        operations::execute(
            session,
            plan,
            &AtomicBool::new(false),
            &check,
            &|_| {},
            &|_, _| {}
        )
        .unwrap()
        .code,
        0
    );
    input.name = format!("{name}-failed");
    input.command = vec!["/nonexistent-executable".into()];
    create::prepare(session, &engine, &input, &check).unwrap();
    let created = std::sync::Mutex::new(String::new());
    let result = create::execute(
        session,
        &engine.endpoint,
        &input,
        &uuid::Uuid::new_v4().to_string(),
        &AtomicBool::new(false),
        &check,
        &|id| *created.lock().unwrap() = id.into(),
    )
    .unwrap();
    assert_ne!(result.code, 0);
    let failed = created.into_inner().unwrap();
    assert!(!failed.is_empty());
    assert!(inspect::container(session, &engine.endpoint, &failed, &check).is_ok());
    run(
        session,
        &Action::Remove.command(&engine.endpoint, &failed).unwrap(),
    );
    sftp.unlink(Path::new(&path)).unwrap();
    assert!(!sftp
        .readdir(Path::new("/tmp"))
        .unwrap()
        .iter()
        .any(|(p, _)| p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .starts_with(".sodeck-")));
}
