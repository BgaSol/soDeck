//! Short local core calls never allocate a thread per subscription/update.
use std::sync::{mpsc, Mutex, OnceLock, Arc};
use std::sync::atomic::{AtomicUsize,Ordering};
pub type Job=Box<dyn FnOnce()+Send+'static>;
struct Pool {normal:mpsc::SyncSender<Job>,control:mpsc::SyncSender<Job>}
fn queue(count:usize,capacity:usize)->mpsc::SyncSender<Job>{
    let (sender,receiver)=mpsc::sync_channel::<Job>(capacity);
    let receiver=Arc::new(Mutex::new(receiver));
    for _ in 0..count {
        let receiver=receiver.clone();
        std::thread::spawn(move||loop{
            let job={receiver.lock().unwrap().recv()};
            match job {Ok(job)=>job(),Err(_)=>break}
        });
    }
    sender
}
pub fn short(method:&str)->bool {
    if observation(method){return true;}
    if method.starts_with("quality.")||matches!(method,"observability.health.get"|"communication.summary"|"communication.sources"|"communication.connections") {return true;}
    method=="pty.start.get"||method.starts_with("state.")||method.starts_with("directories.")||method.starts_with("pty.observe.")||method.starts_with("net.")
}
pub fn observation(method:&str)->bool{method.starts_with("events.")||method.starts_with("observability.")||method.starts_with("traces.")||method.starts_with("metrics.")||method.starts_with("diagnostics.snapshots.")||method.starts_with("diagnostics.capture")||method.starts_with("evidence.")}
pub fn control(method:&str)->bool {matches!(method,"events.v2.ack"|"events.v2.close"|"evidence.cancel"|"diagnostics.capture.cancel"|"events.v2.cleanup.cancel"|"observability.health.get"|"observability.v2.health.get"|"quality.check.cancel"|"quality.check.get"|"state.close"|"state.ack"|"directories.unregister"|"pty.observe.close"|"net.groups.close"|"net.listeners.close"|"net.streams.close"|"net.operations.cancel"|"net.operations.get")}
pub fn submit(method:&str,job:Job)->bool {
    submit_sized(method,1024,job)
}
pub fn submit_sized(method:&str,bytes:usize,job:Job)->bool {
    static POOL:OnceLock<Pool>=OnceLock::new();
    static OBSERVATION:OnceLock<Pool>=OnceLock::new();
    if observation(method){
        static NORMAL:AtomicUsize=AtomicUsize::new(0);static CONTROL:AtomicUsize=AtomicUsize::new(0);
        struct Permit(&'static AtomicUsize,usize);impl Drop for Permit{fn drop(&mut self){self.0.fetch_sub(self.1,Ordering::SeqCst);}}
        let priority=control(method);let used=if priority{&CONTROL}else{&NORMAL};let charge=bytes.saturating_add(256);let limit=if priority{256*1024}else{2*1024*1024-256*1024};
        if used.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|n|n.checked_add(charge).filter(|n|*n<=limit)).is_err(){return false;}
        let permit=Permit(used,charge);let job:Job=Box::new(move||{let _permit=permit;job();});
        let pool=OBSERVATION.get_or_init(||Pool{normal:queue(2,32),control:queue(8,128)});return (if priority{&pool.control}else{&pool.normal}).try_send(job).is_ok();
    }
    let pool=POOL.get_or_init(||Pool{normal:queue(3,256),control:queue(1,64)});
    let lane=if control(method){&pool.control}else{&pool.normal};
    lane.try_send(job).is_ok()
}
