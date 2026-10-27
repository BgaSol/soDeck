//! Short local core calls never allocate a thread per subscription/update.
use std::sync::{mpsc, Mutex, OnceLock, Arc};
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
    method=="pty.start.get"||method.starts_with("state.")||method.starts_with("directories.")||method.starts_with("pty.observe.")||method.starts_with("net.")
}
pub fn submit(method:&str,job:Job)->bool {
    static POOL:OnceLock<Pool>=OnceLock::new();
    let pool=POOL.get_or_init(||Pool{normal:queue(3,256),control:queue(1,64)});
    let lane=if matches!(method,"state.close"|"state.ack"|"directories.unregister"|"pty.observe.close"|"net.groups.close"|"net.listeners.close"|"net.streams.close"|"net.operations.cancel"|"net.operations.get"){&pool.control}else{&pool.normal};
    lane.try_send(job).is_ok()
}
