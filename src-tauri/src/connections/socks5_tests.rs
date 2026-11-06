use super::*;
use tokio::io::DuplexStream;

async fn proxy(
    mut stream: DuplexStream,
    request: Vec<u8>,
    auth: Option<Vec<u8>>,
    response: Vec<u8>,
) {
    let mut greeting = [0; 3];
    stream.read_exact(&mut greeting).await.unwrap();
    assert_eq!(greeting, [5, 1, if auth.is_some() { 2 } else { 0 }]);
    stream.write_all(&[5, greeting[2]]).await.unwrap();
    if let Some(auth) = auth {
        let mut bytes = vec![0; auth.len()];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(bytes, auth);
        stream.write_all(&[1, 0]).await.unwrap();
    }
    let mut bytes = vec![0; request.len()];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(bytes, request);
    for byte in response {
        stream.write_all(&[byte]).await.unwrap();
        tokio::task::yield_now().await;
    }
    // Refusal tests must finish without waiting for an omitted binding address.
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
}

#[tokio::test]
async fn literal_and_remote_domain_requests_consume_only_the_binding_frame() {
    for (host, request) in [
        ("127.0.0.1", vec![5, 1, 0, 1, 127, 0, 0, 1, 0, 80]),
        (
            "::1",
            [vec![5, 1, 0, 4], vec![0; 15], vec![1, 0, 80]].concat(),
        ),
        (
            "remote.invalid",
            [
                vec![5, 1, 0, 3, 14],
                b"remote.invalid".to_vec(),
                vec![0, 80],
            ]
            .concat(),
        ),
    ] {
        for bound in [
            vec![5, 0, 0, 1, 0, 0, 0, 0, 0, 0],
            [vec![5, 0, 0, 4], vec![0; 18]].concat(),
            vec![5, 0, 0, 3, 2, 0xff, 0xfe, 0, 0],
            vec![5, 0, 0, 3, 0, 0, 0],
        ] {
            let (client, server) = tokio::io::duplex(1024);
            let task = tokio::spawn(proxy(
                server,
                request.clone(),
                None,
                [bound, b"tail".to_vec()].concat(),
            ));
            let mut stream: Stream = Box::new(client);
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                connect(&mut stream, host, 80, None),
            )
            .await
            .unwrap()
            .unwrap();
            let mut tail = [0; 4];
            stream.read_exact(&mut tail).await.unwrap();
            assert_eq!(&tail, b"tail");
            task.abort();
        }
    }
}

#[tokio::test]
async fn password_authentication_preserves_byte_limits_and_empty_password() {
    for (user, password) in [
        ("用".to_owned(), "".to_owned()),
        ("u".repeat(255), "p".repeat(255)),
    ] {
        let (client, server) = tokio::io::duplex(2048);
        let mut auth = vec![1, user.len() as u8];
        auth.extend_from_slice(user.as_bytes());
        auth.push(password.len() as u8);
        auth.extend_from_slice(password.as_bytes());
        let task = tokio::spawn(proxy(
            server,
            vec![5, 1, 0, 1, 127, 0, 0, 1, 0, 80],
            Some(auth),
            vec![5, 0, 0, 1, 0, 0, 0, 0, 0, 0],
        ));
        let mut stream: Stream = Box::new(client);
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            connect(&mut stream, "127.0.0.1", 80, Some((&user, &password))),
        )
        .await
        .unwrap()
        .unwrap();
        task.abort();
    }
}

#[tokio::test]
async fn unoffered_authentication_never_sends_connect_or_credentials() {
    for response in [[5, 0], [5, 255], [5, 0x80], [4, 2]] {
        let (client, mut server) = tokio::io::duplex(1024);
        let task = tokio::spawn(async move {
            let mut greeting = [0; 3];
            server.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting, [5, 1, 2]);
            server.write_all(&response).await.unwrap();
            let mut rest = Vec::new();
            server.read_to_end(&mut rest).await.unwrap();
            assert!(rest.is_empty());
        });
        let mut stream: Stream = Box::new(client);
        let error = connect(&mut stream, "remote.invalid", 80, Some(("u", "p")))
            .await
            .unwrap_err();
        assert_eq!(error.code, "socksAuthentication");
        drop(stream);
        tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn invalid_parameters_fail_before_any_network_write() {
    for (host, user, password, code) in [
        ("", "u".to_owned(), "p".to_owned(), "socksAddress"),
        ("中文", "u".to_owned(), "p".to_owned(), "socksAddress"),
        (
            "remote.invalid",
            "".to_owned(),
            "p".to_owned(),
            "socksCredentials",
        ),
        (
            "remote.invalid",
            "用".repeat(86),
            "p".to_owned(),
            "socksCredentials",
        ),
        (
            "remote.invalid",
            "u".to_owned(),
            "p".repeat(256),
            "socksCredentials",
        ),
    ] {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut stream: Stream = Box::new(client);
        let error = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            connect(&mut stream, host, 80, Some((&user, &password))),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.code, code);
        drop(stream);
        let mut bytes = Vec::new();
        server.read_to_end(&mut bytes).await.unwrap();
        assert!(bytes.is_empty());
    }
}

#[tokio::test]
async fn refusal_codes_are_preserved_without_waiting_for_the_rest_of_the_frame() {
    for code in [1, 2, 3, 4, 5, 6, 7, 8, 255] {
        let (client, server) = tokio::io::duplex(1024);
        let task = tokio::spawn(proxy(
            server,
            vec![5, 1, 0, 1, 127, 0, 0, 1, 0, 80],
            None,
            vec![5, code, 0, 1],
        ));
        let mut stream: Stream = Box::new(client);
        let error = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            connect(&mut stream, "127.0.0.1", 80, None),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.code, "socksConnect");
        assert!(error.message.contains(&format!("响应码 {code}")));
        task.abort();
    }
}

#[tokio::test]
async fn malformed_and_truncated_connect_replies_keep_error_categories() {
    for (reply, code) in [
        (vec![4, 0, 0, 1], "socksProtocol"),
        (vec![5, 0, 1, 1], "socksProtocol"),
        (vec![5, 0, 0, 9], "socksProtocol"),
        (vec![5, 0, 0, 1, 0], "network"),
    ] {
        let (client, server) = tokio::io::duplex(1024);
        let task = tokio::spawn(proxy(
            server,
            vec![5, 1, 0, 1, 127, 0, 0, 1, 0, 80],
            None,
            reply,
        ));
        let mut stream: Stream = Box::new(client);
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            connect(&mut stream, "127.0.0.1", 80, None),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.code, code);
        task.abort();
    }
}

#[tokio::test]
async fn rejected_or_malformed_password_response_never_sends_connect() {
    for response in [[1, 1], [1, 255], [5, 0]] {
        let (client, mut server) = tokio::io::duplex(1024);
        let task = tokio::spawn(async move {
            let mut greeting = [0; 3];
            server.read_exact(&mut greeting).await.unwrap();
            server.write_all(&[5, 2]).await.unwrap();
            let mut auth = [0; 5];
            server.read_exact(&mut auth).await.unwrap();
            assert_eq!(auth, [1, 1, b'u', 1, b'p']);
            server.write_all(&response).await.unwrap();
            let mut rest = Vec::new();
            server.read_to_end(&mut rest).await.unwrap();
            assert!(rest.is_empty(), "CONNECT sent after authentication failure");
        });
        let mut stream: Stream = Box::new(client);
        let error = connect(&mut stream, "remote.invalid", 80, Some(("u", "p")))
            .await
            .unwrap_err();
        assert_eq!(error.code, "socksAuthentication");
        drop(stream);
        task.await.unwrap();
    }
}

#[tokio::test]
async fn every_binding_frame_truncation_fails_without_accepting_partial_success() {
    for frame in [
        vec![5, 0, 0, 1, 0, 0, 0, 0, 0, 0],
        [vec![5, 0, 0, 4], vec![0; 18]].concat(),
        vec![5, 0, 0, 3, 3, b'b', b'n', b'd', 0, 0],
    ] {
        for cutoff in 0..frame.len() {
            let (client, mut server) = tokio::io::duplex(1024);
            let partial = frame[..cutoff].to_vec();
            let task = tokio::spawn(async move {
                let mut greeting = [0; 3];
                server.read_exact(&mut greeting).await.unwrap();
                server.write_all(&[5, 0]).await.unwrap();
                let mut request = [0; 10];
                server.read_exact(&mut request).await.unwrap();
                server.write_all(&partial).await.unwrap();
            });
            let mut stream: Stream = Box::new(client);
            let error = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                connect(&mut stream, "127.0.0.1", 80, None),
            )
            .await
            .unwrap()
            .unwrap_err();
            assert_eq!(error.code, "network", "cutoff {cutoff} of {frame:?}");
            task.await.unwrap();
        }
    }
}
