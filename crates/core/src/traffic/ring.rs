//! A fixed allocation includes both samples and list indices. Compaction never allocates.
use super::registry::Sample;
const NONE:u32=u32::MAX;
#[derive(Clone,Copy)]
struct Node {sample:Sample,next:u32,previous:u32,newer:u32,older:u32,owner:u32}
impl Default for Node {fn default()->Self{Self{sample:Sample::default(),next:NONE,previous:NONE,newer:NONE,older:NONE,owner:NONE}}}
#[derive(Clone,Copy)]
struct List {head:u32,tail:u32}
impl Default for List {fn default()->Self{Self{head:NONE,tail:NONE}}}
pub(super) struct Rings {nodes:Box<[Node]>,lists:Box<[List]>,next_list:u32,free:u32,oldest:u32,newest:u32,len:usize}
impl Rings {
    pub fn new(bytes:usize,series:usize)->Self {
        let count=(bytes-series*std::mem::size_of::<List>())/std::mem::size_of::<Node>();
        assert!(count>series&&count<u32::MAX as usize);
        let mut nodes=vec![Node::default();count].into_boxed_slice();
        for (index,node) in nodes.iter_mut().enumerate(){node.next=if index+1<count{index as u32+1}else{NONE};}
        Self{nodes,lists:vec![List::default();series].into_boxed_slice(),next_list:0,free:0,oldest:NONE,newest:NONE,len:0}
    }
    pub fn create(&mut self)->Option<u32> {
        if self.next_list as usize>=self.lists.len(){return None;}
        let id=self.next_list;self.next_list+=1;Some(id)
    }
    pub fn push(&mut self,owner:u32,sample:Sample) {
        if self.free==NONE {
            let mut candidate=self.oldest;
            while self.nodes[candidate as usize].next==NONE {candidate=self.nodes[candidate as usize].newer;assert_ne!(candidate,NONE);}
            let next=self.nodes[candidate as usize].next;
            let previous=self.nodes[candidate as usize].sample;
            self.nodes[next as usize].sample.merge(previous);
            self.remove(candidate);
        }
        let index=self.free;self.free=self.nodes[index as usize].next;
        let list=&mut self.lists[owner as usize];
        self.nodes[index as usize]=Node{sample,next:NONE,previous:list.tail,newer:NONE,older:self.newest,owner};
        if list.tail!=NONE{self.nodes[list.tail as usize].next=index;}else{list.head=index;}
        list.tail=index;
        if self.newest!=NONE{self.nodes[self.newest as usize].newer=index;}else{self.oldest=index;}
        self.newest=index;self.len+=1;
    }
    fn remove(&mut self,index:u32) {
        let node=self.nodes[index as usize];let list=&mut self.lists[node.owner as usize];
        if node.previous!=NONE{self.nodes[node.previous as usize].next=node.next;}else{list.head=node.next;}
        if node.next!=NONE{self.nodes[node.next as usize].previous=node.previous;}else{list.tail=node.previous;}
        if node.older!=NONE{self.nodes[node.older as usize].newer=node.newer;}else{self.oldest=node.newer;}
        if node.newer!=NONE{self.nodes[node.newer as usize].older=node.older;}else{self.newest=node.older;}
        self.nodes[index as usize].next=self.free;self.free=index;self.len-=1;
    }
    pub fn expire(&mut self,before:u64) {
        for id in 0..self.next_list {
            while self.lists[id as usize].head!=NONE {
                let head=self.lists[id as usize].head;
                if self.nodes[head as usize].sample.timestamp>=before{break;}
                self.remove(head);
            }
        }
    }
    pub fn iter(&self,id:Option<u32>)->impl Iterator<Item=&Sample> {
        let mut next=id.map_or(NONE,|id|self.lists[id as usize].head);
        std::iter::from_fn(move||{if next==NONE{return None;}let node=&self.nodes[next as usize];next=node.next;Some(&node.sample)})
    }
    #[cfg(test)]fn bytes(&self)->usize{self.nodes.len()*std::mem::size_of::<Node>()+self.lists.len()*std::mem::size_of::<List>()}
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn fixed_pool_reuses_slots_without_losing_sums_or_short_peaks(){
        let mut rings=Rings::new(1024,3);let a=rings.create().unwrap();let b=rings.create().unwrap();
        for timestamp in 1..1000 {rings.push(a,Sample::new(timestamp*1000,1000,if timestamp==7{10000}else{1},2,false,false));rings.push(b,Sample::new(timestamp*1000,1000,3,4,false,false));}
        assert!(rings.bytes()<=1024);
        assert_eq!(rings.iter(Some(a)).map(|s|s.sent).sum::<u64>(),10998);
        assert_eq!(rings.iter(Some(b)).map(|s|s.sent).sum::<u64>(),2997);
        assert_eq!(rings.iter(Some(a)).map(|s|s.peak_send).fold(0.0,f64::max),10000.0);
        assert_eq!(rings.iter(Some(a)).map(|s|s.valid_ms).sum::<u64>(),999000);
        rings.expire(1000_000);assert_eq!(rings.len,0);
        rings.push(a,Sample::new(1000_001,1000,10,20,false,false));assert_eq!(rings.iter(Some(a)).count(),1);
    }
}
