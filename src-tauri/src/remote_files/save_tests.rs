//! Controlled SFTP wire replies at the irreversible commit boundary.
use super::*;
use tokio::io::{AsyncReadExt, DuplexStream};

async fn packet(stream: &mut DuplexStream) -> Vec<u8> {
    let length = stream.read_u32().await.unwrap() as usize;
    assert!(length < 4096);
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await.unwrap();
    bytes
}
async fn send(stream: &mut DuplexStream, bytes: &[u8]) {
    stream.write_u32(bytes.len() as u32).await.unwrap();
    stream.write_all(bytes).await.unwrap();
}
async fn status(stream: &mut DuplexStream, request: &[u8], code: StatusCode) {
    let mut bytes = vec![101];
    bytes.extend_from_slice(&request[1..5]);
    bytes.extend_from_slice(&(code as u32).to_be_bytes());
    ssh_string(&mut bytes, "injected result");
    ssh_string(&mut bytes, "en");
    send(stream, &bytes).await;
}

async fn commit_reply(code: Option<StatusCode>) -> Result<Option<FileAttributes>, SaveFailure> {
    let (client, mut server) = tokio::io::duplex(4096);
    let (arrived_tx, arrived_rx) = tokio::sync::oneshot::channel();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        assert_eq!(packet(&mut server).await[0], 1);
        send(&mut server, &[2, 0, 0, 0, 3]).await;
        let commit = packet(&mut server).await;
        assert_eq!(commit[0], 200);
        let mut expected = vec![];
        ssh_string(&mut expected, "posix-rename@openssh.com");
        ssh_string(&mut expected, "/temporary");
        ssh_string(&mut expected, "/destination");
        assert_eq!(&commit[5..], expected);
        arrived_tx.send(()).unwrap();
        reply_rx.await.unwrap();
        let Some(code) = code else { return };
        status(&mut server, &commit, code).await;
        if code == StatusCode::Ok {
            let stat = packet(&mut server).await;
            assert_eq!(stat[0], 17);
            status(&mut server, &stat, StatusCode::Failure).await;
        } else if code == StatusCode::PermissionDenied {
            let remove = packet(&mut server).await;
            assert_eq!(remove[0], 13);
            let mut expected = vec![];
            ssh_string(&mut expected, "/temporary");
            assert_eq!(
                &remove[5..],
                expected,
                "Only this job's temporary file may be removed"
            );
            status(&mut server, &remove, StatusCode::Ok).await;
        }
        // A disconnect/uncertain response must not trigger another rename or cleanup.
        let mut trailing = vec![];
        server.read_to_end(&mut trailing).await.unwrap();
        assert!(
            trailing.is_empty(),
            "Unexpected follow-up mutation: {trailing:?}"
        );
    });
    let raw = RawSftpSession::new_with_config(
        client,
        russh_sftp::client::Config {
            request_timeout_secs: 2,
            ..Default::default()
        },
    );
    raw.init().await.unwrap();
    let work = tokio::spawn(async move {
        let result = commit_file(&raw, "/temporary", "/destination").await;
        let _ = raw.close_session();
        result
    });
    arrived_rx.await.unwrap();
    assert!(
        !work.is_finished(),
        "Sending the commit is not a completion receipt"
    );
    reply_tx.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    result
}

#[tokio::test]
async fn acknowledged_commit_survives_followup_stat_failure() {
    assert!(commit_reply(Some(StatusCode::Ok)).await.unwrap().is_none());
}
#[tokio::test]
async fn lost_commit_reply_is_unknown_and_never_retried() {
    assert!(commit_reply(None).await.unwrap_err().unknown);
    assert!(
        commit_reply(Some(StatusCode::ConnectionLost))
            .await
            .unwrap_err()
            .unknown
    );
}
#[tokio::test]
async fn explicit_commit_rejection_only_cleans_its_temporary_file() {
    assert!(
        !commit_reply(Some(StatusCode::PermissionDenied))
            .await
            .unwrap_err()
            .unknown
    );
}
