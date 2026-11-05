use serde_json::json;
use sodeck::streams::{Frame, Replay, Sink};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

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
