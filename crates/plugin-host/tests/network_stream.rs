use serde_json::{json,Value};
use sodeck_core::{Core,Caller,WorkspaceScope};
use sodeck_plugin_host::{PluginProcess,BinaryCallbacks,RpcError};
use std::{sync::Arc,io::{Read,Write},net::{TcpStream,Shutdown},time::{Duration,Instant},process::Command};

struct TestWorker { process:PluginProcess,core:Arc<Core>,caller:Caller,_dir:tempfile::TempDir }
impl TestWorker {
    fn new()->Self {
        let dir=tempfile::tempdir().unwrap();let worker=dir.path().join("network-worker.js");
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut command=Command::new("node");
        command.arg(root.join("node_modules/esbuild/bin/esbuild")).arg(root.join("crates/plugin-host/tests/network-stream-worker.ts"))
            .args(["--bundle","--format=iife","--platform=neutral","--target=es2022"]).arg(format!("--outfile={}",worker.display()));
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
        let output=command.output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
        let core=Arc::new(Core::open(dir.path().join("data")).unwrap());
        let caller=Caller{source:"plugin".into(),plugin_id:Some("test.binary".into()),plugin_version:Some("0.0.0".into()),instance_id:"binary-instance".into(),view_id:None,mount_generation:None};
        core.register_workspace("binary-test","one");core.register_network_view(&caller.instance_id,"binary-view",WorkspaceScope{workspace_id:"binary-test".into(),workspace_generation:"one".into()}).unwrap();
        let (c,owner)=(core.clone(),caller.clone());let callback=Arc::new(move|method:&str,p|c.call(&owner,method,p).map_err(|e|RpcError::new(&e.code,e.message)));
        let (c,owner)=(core.clone(),caller.clone());let receive=Arc::new(move|frame|c.network_input(&owner,frame).map_err(|e|RpcError::new(&e.code,e.message)));
        let (c,owner)=(core.clone(),caller.clone());let produce=Arc::new(move|sink:&mut sodeck_plugin_host::BinarySink|c.network_output(&owner,|f|sink(f)));
        let process=PluginProcess::spawn_with_binary(env!("CARGO_BIN_EXE_sodeck-plugin-host"),&worker,callback,Some(BinaryCallbacks{receive,produce})).unwrap();
        process.initialize(json!({})).unwrap();Self{process,core,caller,_dir:dir}
    }
    fn start(&self,mode:&str)->u16 {self.process.invoke("start",json!({"mode":mode})).unwrap()["port"].as_u64().unwrap() as u16}
    fn stats(&self)->Value {self.process.invoke("stats",json!({})).unwrap()}
}
impl Drop for TestWorker {fn drop(&mut self){self.process.terminate();self.core.close_instance(&self.caller.instance_id);}}
fn connect(port:u16)->TcpStream {
    let stream=TcpStream::connect(("127.0.0.1",port)).unwrap();stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();stream.set_write_timeout(Some(Duration::from_secs(10))).unwrap();stream
}

#[test]
fn real_quickjs_typed_arrays_stream_without_json_payloads() {
    let worker=TestWorker::new();let port=worker.start("echo");let mut socket=connect(port);
    let pattern:Vec<_>=(0..65536).map(|n|(n%251) as u8).collect();let mut received=vec![0;pattern.len()];
    for _ in 0..32 {socket.write_all(&pattern).unwrap();socket.read_exact(&mut received).unwrap();assert_eq!(received,pattern);}
    socket.shutdown(Shutdown::Write).unwrap();let mut end=[0;1];assert_eq!(socket.read(&mut end).unwrap(),0);
    let stats=worker.stats();assert_eq!(stats["failure"],"");assert_eq!(stats["bytes"],2*1024*1024);
    worker.process.stop().unwrap();
    assert!(TcpStream::connect(("127.0.0.1",port)).is_err());
}

#[test]
fn real_worker_splice_keeps_same_packet_tail_and_reverse_half_close() {
    let worker=TestWorker::new();let port=worker.start("splice");let mut a=connect(port);
    a.write_all(b"HELLOexact-tail").unwrap();std::thread::sleep(Duration::from_millis(30));let mut b=connect(port);
    let mut ready=[0;5];a.read_exact(&mut ready).unwrap();assert_eq!(&ready,b"READY");
    let mut tail=[0;10];b.read_exact(&mut tail).unwrap();assert_eq!(&tail,b"exact-tail");
    a.shutdown(Shutdown::Write).unwrap();let mut end=[0;1];assert_eq!(b.read(&mut end).unwrap(),0);
    b.write_all(b"return").unwrap();let mut reply=[0;6];a.read_exact(&mut reply).unwrap();assert_eq!(&reply,b"return");
    assert_eq!(worker.stats()["failure"],"");
    let started=Instant::now();worker.process.terminate();let terminated=started.elapsed();worker.core.close_instance(&worker.caller.instance_id);
    let closed=started.elapsed();assert!(TcpStream::connect(("127.0.0.1",port)).is_err());assert!(closed<Duration::from_secs(1),"terminate={terminated:?}, local close={closed:?}");
}
