use serde_json::json;
use sodeck::streams::{Frame, Replay, Sink};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sodeck-replay-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
fn reader(replay: &Arc<Replay>, token: &str) -> tokio::sync::mpsc::UnboundedReceiver<Frame> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Sink = Arc::new(move |frame| tx.send(frame).map_err(|_| "gone".into()));
    tokio::spawn(replay.clone().subscribe(token.into(), sink, || true));
    rx
}

#[tokio::test]
async fn bounded_replay_waits_for_checkpoint_and_cancellation_releases_slow_output() {
    let replay = Arc::new(Replay::default());
    let cancel = CancellationToken::new();
    replay.push(&vec![1; 512 * 1024], &cancel).await.unwrap();
    let r = replay.clone();
    let c = cancel.clone();
    let mut producer = tokio::spawn(async move { r.push(b"blocked", &c).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut producer)
            .await
            .is_err()
    );
    cancel.cancel();
    assert!(producer.await.unwrap().is_err());
}

#[tokio::test]
async fn checkpoints_are_confirmed_bounded_and_replayed_before_new_bytes() {
    let replay = Arc::new(Replay::default());
    let cancel = CancellationToken::new();
    replay.push(b"first", &cancel).await.unwrap();
    let subscribe = |token: &str| {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Frame>();
        let sink: Sink = Arc::new(move |f| tx.send(f).map_err(|_| "gone".into()));
        tokio::spawn(replay.clone().subscribe(token.into(), sink, || true));
        rx
    };
    let mut reader = subscribe("first");
    loop {
        let f = reader.recv().await.unwrap();
        assert!(replay.ack("other", f.packet).is_err());
        replay.ack("first", f.packet).unwrap();
        assert!(replay.ack("first", f.packet).is_err());
        if f.kind == "data" {
            assert_eq!(f.data, b"first");
            break;
        }
    }
    assert!(replay
        .checkpoint("first", 6, vec![], json!({}), true, true)
        .is_err());
    replay
        .checkpoint(
            "first",
            5,
            b"opaque screen".to_vec(),
            json!({"cols":80}),
            true,
            true,
        )
        .unwrap();
    replay.detach("first");
    replay.push(b"next", &cancel).await.unwrap();
    let mut second = subscribe("second");
    let mut snapshot = vec![];
    loop {
        let f = second.recv().await.unwrap();
        replay.ack("second", f.packet).unwrap();
        if f.kind == "snapshot" {
            snapshot.extend(f.data);
        } else if f.kind == "data" {
            assert_eq!(f.offset, 9);
            assert_eq!(f.data, b"next");
            break;
        }
    }
    assert_eq!(snapshot, b"opaque screen");
    replay.detach("second");
}

#[tokio::test]
async fn journal_remount_replays_over_512k_and_commits_a_large_snapshot_with_independent_ui() {
    let root = directory();
    let replay = Arc::new(Replay::journaled(root.clone()).unwrap());
    let cancel = CancellationToken::new();
    let original: Vec<_> = (0..3 * 1024 * 1024).map(|n| (n % 251) as u8).collect();
    replay.push(&original, &cancel).await.unwrap();
    let mut first = reader(&replay, "first");
    let mut bytes = vec![];
    while bytes.len() < original.len() {
        let frame = first.recv().await.unwrap();
        replay.ack("first", frame.packet).unwrap();
        if frame.kind == "data" {
            bytes.extend(frame.data);
        }
    }
    assert_eq!(bytes, original);
    assert!(replay.metadata("other", json!({})).is_err());
    replay
        .metadata("first", json!({"search":{"query":"timeout","open":true}}))
        .unwrap();
    let snapshot = vec![b'S'; 3 * 1024 * 1024];
    for (i, chunk) in snapshot.chunks(16384).enumerate() {
        replay
            .checkpoint(
                "first",
                original.len() as u64,
                chunk.to_vec(),
                json!({"cols":120}),
                i == 0,
                (i + 1) * 16384 == snapshot.len(),
            )
            .unwrap();
    }
    replay
        .checkpoint(
            "first",
            original.len() as u64,
            b"incomplete".to_vec(),
            json!({"cols":80}),
            true,
            false,
        )
        .unwrap();
    replay.detach("first");
    drop(first);
    replay.push(b"new output", &cancel).await.unwrap();
    let mut second = reader(&replay, "second");
    let mut restored = vec![];
    loop {
        let frame = second.recv().await.unwrap();
        replay.ack("second", frame.packet).unwrap();
        if frame.kind == "reset" {
            assert_eq!(frame.offset, original.len() as u64);
            assert_eq!(frame.metadata["cols"], 120);
            assert_eq!(frame.metadata["ui"]["search"]["query"], "timeout");
        }
        if frame.kind == "snapshot" {
            restored.extend_from_slice(&frame.data);
        }
        if frame.kind == "data" {
            assert_eq!(frame.data, b"new output");
            break;
        }
    }
    assert_eq!(restored, snapshot);
    replay.detach("second");
    drop(second);
    drop(replay);
    tokio::time::timeout(Duration::from_secs(2), async {
        while root.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn journal_failure_reports_degraded_recovery_but_delivers_live_output() {
    let root = directory();
    let replay = Arc::new(Replay::journaled(root.clone()).unwrap());
    let cancel = CancellationToken::new();
    let mut first = reader(&replay, "live");
    loop {
        let frame = first.recv().await.unwrap();
        replay.ack("live", frame.packet).unwrap();
        if frame.kind == "caughtUp" {
            break;
        }
    }
    std::fs::remove_dir_all(&root).unwrap();
    replay.push(b"live output", &cancel).await.unwrap();
    let mut warning = false;
    let mut delivered = false;
    while !warning || !delivered {
        let frame = first.recv().await.unwrap();
        replay.ack("live", frame.packet).unwrap();
        if frame.kind == "recovery" {
            warning = true;
            assert_eq!(frame.metadata["available"], false);
        }
        if frame.kind == "data" {
            assert_eq!(frame.data, b"live output");
            delivered = true;
        }
    }
    assert!(warning);
    std::fs::create_dir_all(&root).unwrap();
    replay
        .checkpoint(
            "live",
            11,
            b"complete display".to_vec(),
            json!({"cols":80}),
            true,
            true,
        )
        .unwrap();
    replay.push(b"resumed", &cancel).await.unwrap();
    let mut recovered = false;
    let mut delivered = false;
    while !recovered || !delivered {
        let frame = first.recv().await.unwrap();
        replay.ack("live", frame.packet).unwrap();
        if frame.kind == "recovery" {
            recovered = true;
            assert_eq!(frame.metadata["available"], true);
        }
        if frame.kind == "data" {
            assert_eq!(frame.data, b"resumed");
            delivered = true;
        }
    }
    assert!(recovered);
    replay.detach("live");
}

#[tokio::test]
async fn a_new_checkpoint_does_not_prune_bytes_still_needed_by_a_remount_reader() {
    let replay = Arc::new(Replay::journaled(directory()).unwrap());
    let cancel = CancellationToken::new();
    let first_bytes = vec![b'A'; 2 * 1024 * 1024];
    replay.push(&first_bytes, &cancel).await.unwrap();
    let mut fast = reader(&replay, "fast");
    let mut consumed = 0;
    while consumed < first_bytes.len() {
        let frame = fast.recv().await.unwrap();
        replay.ack("fast", frame.packet).unwrap();
        if frame.kind == "data" {
            consumed += frame.data.len();
        }
    }
    let mut slow = reader(&replay, "slow");
    let reset = slow.recv().await.unwrap();
    assert_eq!(reset.kind, "reset");
    assert_eq!(reset.offset, 0);
    replay
        .checkpoint(
            "fast",
            consumed as u64,
            b"snapshot".to_vec(),
            json!({}),
            true,
            true,
        )
        .unwrap();
    replay.detach("fast");
    let later = vec![b'B'; 2 * 1024 * 1024];
    replay.push(&later, &cancel).await.unwrap();
    replay.ack("slow", reset.packet).unwrap();
    let mut restored = Vec::new();
    while restored.len() < first_bytes.len() + later.len() {
        let frame = tokio::time::timeout(Duration::from_secs(2), slow.recv())
            .await
            .unwrap()
            .unwrap();
        assert_ne!(frame.kind, "error");
        replay.ack("slow", frame.packet).unwrap();
        if frame.kind == "data" {
            restored.extend_from_slice(&frame.data);
        }
    }
    assert_eq!(&restored[..first_bytes.len()], first_bytes);
    assert_eq!(&restored[first_bytes.len()..], later);
    replay.detach("slow");
}
