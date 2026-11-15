#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn session_stream_recovers_raw_bytes_and_cancels_only_its_channel() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let f=Fixture::new();let target=SshServer::new().await;let server=f.server("127.0.0.1",target.addr.port(),json!([]));let context=connected(&f,&server).await.context.unwrap();let (_,token,terminal)=terminal_fixture(&f);
    let invoke=|method:&str,p:Value|f.core.ui_event("main",&token,json!({"rpc":method,"params":p})).unwrap();
    let input=json!({"expectedContext":context,"operationId":"binary","sequence":"1","command":"status-test:binary","maxOutputBytes":100000,"retention":"fixed"});
    let start=invoke("session.stream.open",input.clone());let id=start["streamId"].as_str().unwrap();assert_eq!(invoke("session.stream.open",input.clone())["streamId"],id);
    assert_eq!(invoke("session.stream.lookup",input.clone())["streamId"],id);let mut unknown=input.clone();unknown["operationId"]=json!("never-opened");assert_eq!(invoke("session.stream.lookup",unknown)["known"],false);
    let mut conflict=input.clone();conflict["command"]=json!("sample");assert!(invoke("session.stream.open",conflict)["rpcError"].is_object());
    let mut conflict=input.clone();conflict["maxOutputBytes"]=json!(100001);assert!(invoke("session.stream.lookup",conflict)["rpcError"].is_object());
    let deadline=std::time::Instant::now()+Duration::from_secs(5);let status=loop{let s=invoke("session.stream.status",json!({"streamId":id}));if s["finished"]==true{break s;}assert!(std::time::Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(20)).await;};
    assert_eq!(status["state"],"completed");assert_eq!(status["remoteExitConfirmed"],true);
    let mut offset=0u64;let end=status["producedBytes"].as_str().unwrap().parse::<u64>().unwrap();let mut output=vec![];
    while offset<end{let p=json!({"id":id,"action":"read","offset":offset.to_string(),"watermark":end.to_string()});let r=f.core.session_stream_data("main",&token,p.clone(),None).await.unwrap();assert_eq!(r,f.core.session_stream_data("main",&token,p,None).await.unwrap());let b:Vec<u8>=serde_json::from_value(r["data"].clone()).unwrap();assert!(b.len()<=16384);offset+=b.len() as u64;output.extend(b);}
    let (mut stdout,mut stderr)=(vec![],vec![]);for line in output.split(|b|*b==b'\n').filter(|b|!b.is_empty()){let envelope:Value=serde_json::from_slice(line).unwrap();let bytes=STANDARD.decode(envelope["dataBase64"].as_str().unwrap()).unwrap();if envelope["source"]=="stdout"{stdout.extend(bytes)}else{stderr.extend(bytes)}}
    assert_eq!(stdout,(0..16385).map(|i|i as u8).collect::<Vec<_>>());assert_eq!(stderr,(0..8193).map(|i|(255-i%256) as u8).collect::<Vec<_>>());
    invoke("session.stream.release",json!({"streamId":id}));assert!(invoke("session.stream.open",input)["rpcError"].is_object());
    let hang=invoke("session.stream.open",json!({"expectedContext":context,"operationId":"hang","sequence":"2","command":"status-test:hang","maxOutputBytes":1024,"retention":"rolling"}));let id=hang["streamId"].as_str().unwrap();
    let large=invoke("session.stream.open",json!({"expectedContext":context,"operationId":"quota","sequence":"3","command":"status-test:hang","maxOutputBytes":48*1024*1024,"retention":"fixed"}));let large_id=large["streamId"].as_str().unwrap();assert!(invoke("session.stream.open",json!({"expectedContext":context,"operationId":"over-quota","sequence":"4","command":"status-test:hang","maxOutputBytes":32*1024*1024,"retention":"fixed"}))["rpcError"].is_object());invoke("session.stream.release",json!({"streamId":large_id}));
    let (tx,mut rx)=tokio::sync::mpsc::unbounded_channel();let sink:Sink=Arc::new(move|frame|tx.send(frame).map_err(|_|"closed".into()));f.core.session_stream_data("main",&token,json!({"action":"attach","id":id}),Some(sink)).await.unwrap();let frame=rx.recv().await.unwrap();assert_eq!(frame.kind,"reset");
    f.core.session_stream_data("main",&token,json!({"action":"ack","id":id,"packet":frame.packet}),None).await.unwrap();
    invoke("session.stream.close",json!({"streamId":id}));tokio::time::sleep(Duration::from_millis(100)).await;assert_eq!(invoke("session.stream.status",json!({"streamId":id}))["state"],"cancelled");invoke("session.stream.release",json!({"streamId":id}));
    let rolling=invoke("session.stream.open",json!({"expectedContext":context,"operationId":"pinned-binary","sequence":"5","command":"status-test:binary","maxOutputBytes":100000,"retention":"rolling"}));let id=rolling["streamId"].as_str().unwrap();
    let deadline=std::time::Instant::now()+Duration::from_secs(5);let status=loop{let status=invoke("session.stream.status",json!({"streamId":id}));if status["finished"]==true{break status;}assert!(std::time::Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(20)).await;};
    let pin=f.core.session_stream_data("main",&token,json!({"action":"pin","id":id,"start":"0","end":status["producedBytes"]}),None).await.unwrap();
    let fixed=json!({"expectedContext":context,"operationId":"fixed-after-pin","sequence":"6","command":"status-test:hang","maxOutputBytes":64*1024*1024,"retention":"fixed"});assert!(invoke("session.stream.open",fixed.clone())["rpcError"].is_object(),"fixed captures and protected rolling bytes share the 64MiB reading quota");
    f.core.session_stream_data("main",&token,json!({"action":"unpin","id":id,"pinId":pin["pinId"]}),None).await.unwrap();let freed=invoke("session.stream.open",fixed);assert!(freed["streamId"].is_string(),"{freed}");invoke("session.stream.release",json!({"streamId":freed["streamId"]}));invoke("session.stream.release",json!({"streamId":id}));
    td(&f.core,"main",&token,json!({"op":"write","id":terminal,"sequence":0,"data":b"still-connected"})).await.unwrap();assert_eq!(target.state.auth.load(Ordering::Relaxed),1);close_terminal_views(&f);f.core.shutdown().unwrap();
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn slow_stream_ack_does_not_block_capture_and_revoked_readers_cannot_resume() {
    let f=Fixture::new();let ssh=SshServer::new().await;let server=f.server("127.0.0.1",ssh.addr.port(),json!([]));let context=connected(&f,&server).await.context.unwrap();let (view,token,_)=terminal_fixture(&f);
    let opened=f.core.ui_event("main",&token,json!({"rpc":"session.stream.open","params":{"expectedContext":context,"operationId":"burst","sequence":"1","command":"status-test:stream-burst","maxOutputBytes":65536,"retention":"rolling"}})).unwrap();let id=opened["streamId"].as_str().unwrap();
    let (tx,mut rx)=tokio::sync::mpsc::unbounded_channel();let sink:Sink=Arc::new(move|frame|tx.send(frame).map_err(|_|"closed".into()));
    f.core.session_stream_data("main",&token,json!({"action":"attach","id":id,"offset":"0"}),Some(sink)).await.unwrap();
    let reset=tokio::time::timeout(Duration::from_secs(3),rx.recv()).await.unwrap().unwrap();assert_eq!(reset.kind,"reset");
    tokio::time::sleep(Duration::from_millis(200)).await;assert!(rx.try_recv().is_err());
    let deadline=std::time::Instant::now()+Duration::from_secs(10);loop { let status=f.core.ui_event("main",&token,json!({"rpc":"session.stream.status","params":{"streamId":id}})).unwrap();if status["finished"]==true {assert_eq!(status["state"],"completed");assert!(status["producedBytes"].as_str().unwrap().parse::<u64>().unwrap()>3_000_000);break;}assert!(std::time::Instant::now()<deadline,"{status}");tokio::time::sleep(Duration::from_millis(25)).await; }
    // A real stalled reader keeps one pending packet while the source completes and rotates.
    tokio::time::sleep(Duration::from_secs(30)).await;assert!(rx.try_recv().is_err());
    f.core.session_stream_data("main",&token,json!({"action":"ack","id":id,"packet":reset.packet}),None).await.unwrap();let gap=tokio::time::timeout(Duration::from_secs(3),rx.recv()).await.unwrap().unwrap();assert_eq!(gap.kind,"gap");
    tokio::time::sleep(Duration::from_secs(2)).await;assert!(rx.try_recv().is_err());
    f.core.ui_detach("main",&token);assert!(f.core.session_stream_data("main",&token,json!({"action":"ack","id":id,"packet":gap.packet}),None).await.is_err());
    let mounted=f.core.ui_attach("main",&view).unwrap();let replacement=mounted["surfaceId"].as_str().unwrap();f.core.ui_surface_ready("main",replacement).unwrap();
    let (tx,mut rx)=tokio::sync::mpsc::unbounded_channel();let sink:Sink=Arc::new(move|frame|tx.send(frame).map_err(|_|"closed".into()));
    f.core.session_stream_data("main",replacement,json!({"action":"attach","id":id}),Some(sink)).await.unwrap();let reset=tokio::time::timeout(Duration::from_secs(3),rx.recv()).await.unwrap().unwrap();assert_eq!(reset.kind,"reset");
    f.core.session_stream_data("main",replacement,json!({"action":"ack","id":id,"packet":reset.packet}),None).await.unwrap();let data=tokio::time::timeout(Duration::from_secs(3),rx.recv()).await.unwrap().unwrap();assert_eq!(data.kind,"data");assert!(data.data.len()<=16384);
    close_terminal_views(&f);f.core.shutdown().unwrap();
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn content_export_is_sequential_atomic_and_owned_by_the_surface() {
    let f=Fixture::new();let target=SshServer::new().await;let server=f.server("127.0.0.1",target.addr.port(),json!([]));connected(&f,&server).await;let (_,token,_)=terminal_fixture(&f);
    let target_path=f.path.join("output.txt");std::fs::write(&target_path,b"before").unwrap();
    let begin=|request:&str|f.core.content_export_begin("main",&token,&json!({"requestId":request,"name":"output.txt","byteLength":"6"})).unwrap()["id"].as_str().unwrap().to_string();
    let id=begin("cancel");f.core.content_export_choose(&id,Ok(Some(target_path.clone())));let p=json!({"id":id,"action":"append","offset":"0","data":b"abc"});
    let a=f.core.content_export_data("main",&token,p.clone()).await.unwrap();assert_eq!(a,f.core.content_export_data("main",&token,p).await.unwrap());
    assert!(f.core.content_export_data("other",&token,json!({"id":id,"action":"status"})).await.is_err());f.core.content_export_data("main",&token,json!({"id":id,"action":"cancel"})).await.unwrap();assert_eq!(std::fs::read(&target_path).unwrap(),b"before");
    let id=begin("changed");f.core.content_export_choose(&id,Ok(Some(target_path.clone())));f.core.content_export_data("main",&token,json!({"id":id,"action":"append","offset":"0","data":b"abcdef"})).await.unwrap();std::fs::write(&target_path,b"external change").unwrap();let status=f.core.content_export_data("main",&token,json!({"id":id,"action":"finish"})).await.unwrap();assert_eq!(status["state"],"failed");assert_eq!(std::fs::read(&target_path).unwrap(),b"external change");
    let id=begin("finish");f.core.content_export_choose(&id,Ok(Some(target_path.clone())));f.core.content_export_data("main",&token,json!({"id":id,"action":"append","offset":"0","data":b"abcdef"})).await.unwrap();let status=f.core.content_export_data("main",&token,json!({"id":id,"action":"finish"})).await.unwrap();assert_eq!(status["state"],"completed","{status}");assert_eq!(std::fs::read(&target_path).unwrap(),b"abcdef");
    assert!(!std::fs::read_dir(&f.path).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().ends_with(".part")));close_terminal_views(&f);f.core.shutdown().unwrap();
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn content_export_picker_and_commit_failures_preserve_existing_files() {
    let f=Fixture::new();let ssh=SshServer::new().await;let server=f.server("127.0.0.1",ssh.addr.port(),json!([]));connected(&f,&server).await;let (_,token,_)=terminal_fixture(&f);
    let begin=|request:&str|f.core.content_export_begin("main",&token,&json!({"requestId":request,"name":"中文 output.txt","byteLength":"6"})).unwrap()["id"].as_str().unwrap().to_string();
    let target=f.path.join("中文 output.txt");std::fs::write(&target,b"original").unwrap();
    let id=begin("picker-cancel");f.core.content_export_choose(&id,Ok(None));f.core.content_export_choose(&id,Ok(Some(target.clone())));
    assert_eq!(f.core.content_export_data("main",&token,json!({"id":id,"action":"status"})).await.unwrap()["state"],"cancelled");
    let id=begin("missing-parent");f.core.content_export_choose(&id,Ok(Some(f.path.join("absent/output.txt"))));assert_eq!(f.core.content_export_data("main",&token,json!({"id":id,"action":"status"})).await.unwrap()["state"],"failed");
    let directory=f.path.join("directory-target");std::fs::create_dir(&directory).unwrap();std::fs::write(directory.join("keep"),b"original").unwrap();
    let id=begin("rename-failure");f.core.content_export_choose(&id,Ok(Some(directory.clone())));f.core.content_export_data("main",&token,json!({"id":id,"action":"append","offset":"0","data":b"abcdef"})).await.unwrap();
    let status=f.core.content_export_data("main",&token,json!({"id":id,"action":"finish"})).await.unwrap();assert_eq!(status["state"],"failed","{status}");assert_eq!(std::fs::read(directory.join("keep")).unwrap(),b"original");
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        let id=begin("locked-target");f.core.content_export_choose(&id,Ok(Some(target.clone())));f.core.content_export_data("main",&token,json!({"id":id,"action":"append","offset":"0","data":b"abcdef"})).await.unwrap();
        let locked=std::fs::OpenOptions::new().read(true).share_mode(0).open(&target).unwrap();
        let status=f.core.content_export_data("main",&token,json!({"id":id,"action":"finish"})).await.unwrap();assert_eq!(status["state"],"failed","{status}");drop(locked);
    }
    assert_eq!(std::fs::read(&target).unwrap(),b"original");assert!(!std::fs::read_dir(&f.path).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().ends_with(".part")));
    let id=begin("retry");f.core.content_export_choose(&id,Ok(Some(target.clone())));f.core.content_export_data("main",&token,json!({"id":id,"action":"append","offset":"0","data":b"abcdef"})).await.unwrap();
    assert_eq!(f.core.content_export_data("main",&token,json!({"id":id,"action":"finish"})).await.unwrap()["state"],"completed");
    assert_eq!(std::fs::read(&target).unwrap(),b"abcdef");close_terminal_views(&f);f.core.shutdown().unwrap();
}
