use super::*;

pub(super) async fn commit_remote(
    raw: &RawSftpSession,
    temp: &str,
    target: &str,
    replace: bool,
) -> Result<(), OperationError> {
    if !replace {
        return raw
            .rename(temp, target)
            .await
            .map(|_| ())
            .map_err(mutation_error);
    }
    let mut data = Vec::new();
    ssh_string(&mut data, temp).map_err(|e| failure("invalidParams", e))?;
    ssh_string(&mut data, target).map_err(|e| failure("invalidParams", e))?;
    match raw.extended("posix-rename@openssh.com", data).await {
        Ok(Packet::Status(status)) if status.status_code == StatusCode::Ok => Ok(()),
        Ok(Packet::Status(status)) => Err(mutation_error(Error::Status(status))),
        Ok(packet) => Err(failure(
            "unknown",
            format!("提交响应无效，请核对目标：{packet:?}"),
        )),
        Err(error) => Err(mutation_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, DuplexStream};
    async fn packet(s: &mut DuplexStream) -> Vec<u8> {
        let n = s.read_u32().await.unwrap();
        let mut p = vec![0; n as usize];
        s.read_exact(&mut p).await.unwrap();
        p
    }
    async fn send(s: &mut DuplexStream, p: &[u8]) {
        s.write_u32(p.len() as u32).await.unwrap();
        s.write_all(p).await.unwrap();
    }
    async fn reply(replace: bool, code: Option<StatusCode>) -> Result<(), OperationError> {
        let (client, mut server) = tokio::io::duplex(4096);
        let peer = tokio::spawn(async move {
            assert_eq!(packet(&mut server).await[0], 1);
            send(&mut server, &[2, 0, 0, 0, 3]).await;
            let request = packet(&mut server).await;
            assert_eq!(request[0], if replace { 200 } else { 18 });
            let mut expected = vec![];
            if replace {
                ssh_string(&mut expected, "posix-rename@openssh.com").unwrap();
            }
            ssh_string(&mut expected, "/temp").unwrap();
            ssh_string(&mut expected, "/target").unwrap();
            assert_eq!(&request[5..], expected);
            if let Some(code) = code {
                let mut response = vec![101];
                response.extend_from_slice(&request[1..5]);
                response.extend_from_slice(&(code as u32).to_be_bytes());
                ssh_string(&mut response, "result").unwrap();
                ssh_string(&mut response, "en").unwrap();
                send(&mut server, &response).await;
                let mut rest = vec![];
                server.read_to_end(&mut rest).await.unwrap();
                assert!(
                    rest.is_empty(),
                    "commit never retries or deletes a destination"
                );
            }
        });
        let raw = RawSftpSession::new_with_config(
            client,
            russh_sftp::client::Config {
                request_timeout_secs: 2,
                ..Default::default()
            },
        );
        raw.init().await.unwrap();
        let result = commit_remote(&raw, "/temp", "/target", replace).await;
        let _ = raw.close_session();
        drop(raw);
        peer.await.unwrap();
        result
    }
    #[tokio::test]
    async fn commit_ack_rejection_and_loss_are_distinct_without_replay() {
        for replace in [false, true] {
            assert!(reply(replace, Some(StatusCode::Ok)).await.is_ok());
            assert_eq!(
                reply(replace, Some(StatusCode::PermissionDenied))
                    .await
                    .unwrap_err()
                    .code,
                "permissionDenied"
            );
            assert_eq!(reply(replace, None).await.unwrap_err().code, "unknown");
        }
    }
}
