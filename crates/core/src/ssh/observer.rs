use super::{Frame,CoreError,Result,STANDARD,Engine,Value,json,VecDeque};
const BUDGET:usize=256*1024;
pub struct Observer {
    pub id:String,
    frames:VecDeque<Frame>,
    bytes:usize,
    floor:u64,
    last:u64,
    pub notify:bool,
    pub end_notified:bool,
}
impl Observer {
    pub fn new(sequence:u64)->Self {Self{id:crate::model::id(),frames:VecDeque::new(),bytes:0,floor:sequence,last:sequence,notify:false,end_notified:false}}
    pub fn push(&mut self,sequence:u64,bytes:&[u8]) {
        self.last=sequence;
        if bytes.len()>BUDGET {self.frames.clear();self.bytes=0;self.floor=sequence;self.notify=true;return;}
        while self.bytes+bytes.len()>BUDGET {if let Some(frame)=self.frames.pop_front(){self.bytes-=frame.bytes.len();self.floor=frame.sequence;}else{break;}}
        self.frames.push_back(Frame{sequence,bytes:bytes.to_vec()});self.bytes+=bytes.len();self.notify=true;
    }
    pub fn read(&mut self,after:u64,closed:bool)->Result<Value>{
        if after>self.last{return Err(CoreError::new("INVALID_CURSOR","观察游标超出输出范围"));}
        let gap=after<self.floor;
        let mut data=Vec::new();let mut next=after.max(self.floor);
        for frame in self.frames.iter().filter(|f|f.sequence>after.max(self.floor)) {
            if data.len()+frame.bytes.len()>64*1024 {break;}
            data.extend_from_slice(&frame.bytes);next=frame.sequence;
        }
        self.notify=next<self.last;
        Ok(json!({"bytes":STANDARD.encode(data),"nextCursor":next.to_string(),"gap":gap,"closed":closed&&next==self.last}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_is_explicit_and_read_is_retryable(){
        let mut observer=Observer::new(0);
        for n in 1..=10 {observer.push(n,&vec![n as u8;32768]);}
        let read=observer.read(0,false).unwrap();assert_eq!(read["gap"],true);assert_eq!(read["nextCursor"],"4");
        assert_eq!(observer.read(0,false).unwrap(),read);
        assert_eq!(observer.bytes,BUDGET);
        assert!(!observer.read(4,false).unwrap()["gap"].as_bool().unwrap());
    }
}
