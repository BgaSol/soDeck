//! A bounded diagnostic channel advanced only by the existing SSH I/O owner.
use crate::{CoreError, Result};
use serde::Serialize;
use ssh2::{Channel, ErrorCode, Session};
use std::{collections::VecDeque, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};

#[derive(Default)]
pub(crate) struct Budget { state:Mutex<(usize,VecDeque<Instant>,usize)> }
struct Permit(Arc<Budget>);
impl Drop for Permit {fn drop(&mut self){self.0.state.lock().unwrap().0-=1;}}
impl Budget {
    fn reserve(self:&Arc<Self>)->Result<Permit> {
        let mut state=self.state.lock().unwrap();
        if state.0>=8{return Err(CoreError::new("CAPACITY","探测通道名额已满，正在等待释放"));}
        state.0+=1;Ok(Permit(self.clone()))
    }
    fn send(&self,now:Instant)->bool {
        let mut state=self.state.lock().unwrap();
        while state.1.front().is_some_and(|t|now.duration_since(*t)>=Duration::from_secs(1)){state.1.pop_front();}
        if state.1.len()+state.2>=8{return false;}
        state.2+=1;true
    }
    fn sent(&self,now:Instant){let mut state=self.state.lock().unwrap();state.2-=1;state.1.push_back(now);}
    pub fn active(&self)->usize {self.state.lock().unwrap().0}
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Outcome {
    pub observed_at:u64,
    pub outcome:&'static str,
    pub sent:bool,
    pub duration_ms:Option<f64>,
    pub queue_ms:u64,
    pub open_ms:u64,
}
pub(crate) struct Request {
    pub canceled:AtomicBool,
    pub result:Mutex<Option<Outcome>>,
    pub retired:AtomicBool,
    queued:Instant,
}
impl Request {
    pub fn cancel(&self){self.canceled.store(true,Ordering::Release);}
    fn complete(&self,outcome:&'static str,sent:Option<Instant>,opened:Option<Instant>,started:Option<Instant>) {
        let mut result=self.result.lock().unwrap();
        if result.is_some(){return;}
        let now=Instant::now();
        *result=Some(Outcome{observed_at:crate::model::now_ms(),outcome,sent:sent.is_some(),
            duration_ms:(outcome=="succeeded").then(||sent.map(|s|now.duration_since(s).as_secs_f64()*1000.0)).flatten(),
            queue_ms:started.unwrap_or(now).duration_since(self.queued).as_millis()as u64,
            open_ms:started.map_or(0,|s|opened.unwrap_or(now).duration_since(s).as_millis()as u64)});
    }
}
pub(crate) type Slot=Arc<Mutex<Option<Probe>>>;
pub(crate) struct Probe {
    pub request:Arc<Request>,
    _permit:Permit,
    traffic:Option<crate::traffic::Registration>,
    channel:Option<Channel>,
    started:Option<Instant>,
    opened:Option<Instant>,
    sent:Option<Instant>,
    opening:bool,
    sending:bool,
    retiring:bool,
    last_tick:Instant,
}
impl Probe {
    pub fn new(budget:&Arc<Budget>,traffic:Option<crate::traffic::Registration>)->Result<Self> {
        let permit=budget.reserve()?;let now=Instant::now();
        Ok(Self{request:Arc::new(Request{canceled:AtomicBool::new(false),result:Mutex::new(None),retired:AtomicBool::new(false),queued:now}),
            _permit:permit,traffic,channel:None,started:None,opened:None,sent:None,opening:false,sending:false,retiring:false,last_tick:now})
    }
    pub fn opening(&self)->bool {self.opening}
    fn finish(&mut self,code:&'static str){self.request.complete(code,self.sent,self.opened,self.started);self.retiring=true;}
    /// True means all channel state has retired. EAGAIN never releases the permit.
    pub fn tick(&mut self,session:&Session,can_open:bool)->bool {
        let now=Instant::now();let paused=now.duration_since(self.last_tick)>Duration::from_secs(1);self.last_tick=now;
        if self.request.canceled.load(Ordering::Acquire){self.finish("CANCELED");}
        if !self.retiring && (paused || self.sent.is_none()&&now.duration_since(self.request.queued)>=Duration::from_secs(1)) {self.finish("SCHEDULING_DELAY");}
        if self.retiring&&!self.opening&&!self.sending{return self.retire();}
        if self.channel.is_none() {
            if !can_open&&!self.opening{return false;}
            self.started.get_or_insert(now);
            // libssh2's session-wide open state must be resumed with this origin.
            self.opening=true;
            match crate::traffic::origin(session,&self.traffic).and_then(|_|session.channel_session()) {
                Ok(channel)=>{self.channel=Some(channel);self.opening=false;self.opened=Some(Instant::now());},
                Err(e) if again(&e)=>return false,
                Err(e)=>{self.opening=false;self.finish(if e.code()==ErrorCode::Session(-21){"UNSUPPORTED"}else{"DISCONNECTED"});return true;},
            }
        }
        if self.retiring&&!self.sending{return self.retire();}
        if self.sent.is_none() {
            if !self.sending&&!self._permit.0.send(now){return false;}
            self.sending=true;
            match self.channel.as_ref().unwrap().probe_send() {
                Ok(())=>{let time=Instant::now();self._permit.0.sent(time);self.sending=false;self.sent=Some(time);},
                Err(e) if again(&e)=>return false,
                Err(_)=>{self._permit.0.sent(Instant::now());self.sending=false;self.finish("DISCONNECTED");return self.retire();},
            }
        }
        if self.retiring{return self.retire();}
        // A paused local event loop has already been excluded above.
        if self.sent.unwrap().elapsed()>=Duration::from_secs(3){self.finish("CHECK_TIMEOUT");return self.retire();}
        match self.channel.as_ref().unwrap().probe_receive() {
            Ok(_)=>self.finish("succeeded"), // Both SUCCESS and FAILURE match this channel.
            Err(e) if again(&e)=>return false,
            Err(_)=>self.finish("DISCONNECTED"),
        }
        self.retire()
    }
    fn retire(&mut self)->bool {
        let Some(channel)=&mut self.channel else{return true;};
        channel.probe_cancel();
        match channel.close(){Err(e) if again(&e)=>return false,_=>{}}
        match channel.wait_close(){Err(e) if again(&e)=>false,_=>true}
    }
}
impl Drop for Probe {
    fn drop(&mut self){if self.sending{self._permit.0.sent(Instant::now());}self.request.complete(if self.request.canceled.load(Ordering::Acquire){"CANCELED"}else{"DISCONNECTED"},self.sent,self.opened,self.started);self.request.retired.store(true,Ordering::Release);}
}
fn again(e:&ssh2::Error)->bool {e.code()==ErrorCode::Session(-37)}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn retiring_channels_and_sliding_send_window_share_hard_limits(){
        let budget=Arc::new(Budget::default());let mut probes=Vec::new();
        for _ in 0..8{probes.push(Probe::new(&budget,None).unwrap());}
        for probe in &probes{probe.request.cancel();}
        assert!(Probe::new(&budget,None).is_err(),"cancel cannot free an undrained channel");
        probes.pop();assert!(Probe::new(&budget,None).is_ok());drop(probes);assert_eq!(budget.active(),0);
        let now=Instant::now();for _ in 0..8{assert!(budget.send(now));}
        assert!(!budget.send(now+Duration::from_secs(2)),"partially sent requests still reserve the send window");
        for _ in 0..8{budget.sent(now+Duration::from_secs(2));}
        assert!(!budget.send(now+Duration::from_millis(2999)));assert!(budget.send(now+Duration::from_secs(3)));
    }
}
