use serde_json::json;
use sodeck_communication::{Hub,Key};
use sodeck_plugin_host::{PluginProcess,RpcError};
use std::{sync::{Arc,Mutex},time::{Instant,Duration},collections::HashMap};

#[test]
#[ignore="60 second real worker transport pressure test"]
fn sixteen_workers_recover_slow_consumer_without_unbounded_queues(){
    let dir=tempfile::tempdir().unwrap();let worker=dir.path().join("worker.js");
    std::fs::write(&worker,r#"
let slow=false,last=[];
sodeck.onNotification(()=>{throw Error('isolated listener failure');});
sodeck.onNotification(event=>{
 if(event.type!=='state.update')return;
 const p=event.payload;last.push(p);if(last.length>8)last.shift();
 if(!slow)void sodeck.call('state.ack',{subscriptionId:p.subscriptionId,feedEpoch:p.feedEpoch,sequence:p.sequence});
});
globalThis.sodeckPlugin={async invoke(command,p){
 if(command==='open'){slow=p.slow;for(let n=0;n<8;n++){const r=await sodeck.call('state.open',{});await sodeck.call('state.ack',{subscriptionId:r.subscriptionId,feedEpoch:r.feedEpoch,sequence:'0'});}return true;}
 if(command==='resume'){slow=false;for(const r of last)await sodeck.call('state.ack',{subscriptionId:r.subscriptionId,feedEpoch:r.feedEpoch,sequence:r.sequence});return true;}
},stop(){}};
"#).unwrap();
    let hub=Arc::new(Mutex::new(Hub::default()));
    let sent=Arc::new(Mutex::new(HashMap::<String,Instant>::new()));let latencies=Arc::new(Mutex::new(Vec::new()));
    let mut processes=Vec::new();let mut keys=Vec::new();
    for index in 0..16 {
        let owner=index.to_string();let key=Key{contract:"test.v1".into(),scope:"test".into(),object:owner.clone()};
        hub.lock().unwrap().put(key.clone(),json!({}),json!(0)).unwrap();keys.push(key.clone());
        let (h,s,l)=(hub.clone(),sent.clone(),latencies.clone());
        let process=PluginProcess::spawn(env!("CARGO_BIN_EXE_sodeck-plugin-host"),&worker,Arc::new(move|method,p|{
            let mut hub=h.lock().unwrap();
            if method=="state.open" {return hub.open(&owner,&key,Instant::now()).map_err(|e|RpcError::new(e.0,"open"));}
            let id=p["subscriptionId"].as_str().unwrap();
            hub.ack(&owner,id,p["feedEpoch"].as_str().unwrap(),p["sequence"].as_str().unwrap().parse().unwrap()).map_err(|e|RpcError::new(e.0,"ack"))?;
            if index!=0 {if let Some(t)=s.lock().unwrap().remove(id){l.lock().unwrap().push(t.elapsed().as_micros() as u64);}}
            Ok(json!({"accepted":true}))
        })).unwrap();process.initialize(json!({})).unwrap();process.invoke("open",json!({"slow":index==0})).unwrap();processes.push(process);
    }
    assert_eq!(hub.lock().unwrap().stats().subscriptions,128);
    let started=Instant::now();let mut resumed=false;let mut max_queue=0;
    for n in 1..=12000 {
        let target=started+Duration::from_millis(n*5);
        if let Some(wait)=target.checked_duration_since(Instant::now()){std::thread::sleep(wait);}
        if !resumed&&started.elapsed()>=Duration::from_secs(10){processes[0].invoke("resume",json!({})).unwrap();resumed=true;}
        let deliveries={let mut h=hub.lock().unwrap();h.put(keys[n as usize%16].clone(),json!({}),json!({"n":n,"payload":"x".repeat(1024)})).unwrap();max_queue=max_queue.max(h.stats().queue_bytes);h.drain(Instant::now())};
        for d in deliveries {
            sent.lock().unwrap().insert(d.subscription_id.clone(),Instant::now());
            processes[d.owner.parse::<usize>().unwrap()].notify(d.event,d.payload).unwrap();
            hub.lock().unwrap().delivered(&d.subscription_id,d.event);
        }
    }
    for (n,p) in processes.iter().enumerate(){p.stop().unwrap();hub.lock().unwrap().remove_owner(&n.to_string());}
    let mut l=latencies.lock().unwrap();l.sort_unstable();let p95=l[l.len()*95/100];
    println!("COM-13: 16 workers / 128 feeds / 12000 updates; samples={}, ack_p95_us={p95}, max_queue_bytes={max_queue}, elapsed_s={:.2}",l.len(),started.elapsed().as_secs_f64());
    assert!(l.len()>10000);assert!(p95<=200_000);assert!(max_queue<=8*1024*1024);assert_eq!(hub.lock().unwrap().stats().subscriptions,0);
}
