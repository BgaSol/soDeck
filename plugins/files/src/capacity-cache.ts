export type CacheValue<T>={state:'loading'}|{state:'ready';value:T}|{state:'unavailable';message:string};
type Task<T>={promise:Promise<T>;cancel():Promise<unknown>|void};
interface Entry<T>{value:CacheValue<T>;used:number;task?:Task<T>;canceled?:boolean}
/** Visible-only bounded queue. Canceled requests retain their slot until their
 * promise settles, so rapid scrolling cannot exceed the remote I/O budget. */
export class CapacityCache<T>{
  private entries=new Map<string,Entry<T>>();private wanted=new Set<string>();private clock=0;private active=0;private generation=0;
  constructor(private load:(key:string)=>Task<T>,private changed:()=>void,readonly limit=256,readonly concurrency=2){}
  get(key:string){const entry=this.entries.get(key);if(entry)entry.used=++this.clock;return entry?.value;}
  visible(keys:Iterable<string>){
    this.wanted=new Set([...keys].slice(0,this.limit));
    for(const [key,entry] of this.entries){if(!this.wanted.has(key)&&entry.value.state==='loading'){
      if(entry.task){if(!entry.canceled){entry.canceled=true;void entry.task.cancel();}}
      else this.entries.delete(key);
    }}
    for(const key of this.wanted)if(!this.entries.has(key))this.entries.set(key,{value:{state:'loading'},used:++this.clock});
    this.prune();this.pump();
  }
  clear(){this.generation++;this.wanted.clear();for(const [key,e] of this.entries){if(e.task){e.canceled=true;void e.task.cancel();}else this.entries.delete(key);}}
  private prune(){
    const stale=[...this.entries].filter(([key,e])=>!e.task&&!this.wanted.has(key)).sort((a,b)=>a[1].used-b[1].used);
    while(this.entries.size>this.limit&&stale.length)this.entries.delete(stale.shift()![0]);
    if(this.entries.size>this.limit)for(const [key,entry] of [...this.entries].reverse()){
      if(this.entries.size<=this.limit)break;if(!entry.task&&entry.value.state==='loading')this.entries.delete(key);
    }
  }
  private pump(){
    for(const [key,entry] of this.entries){
      if(this.active>=this.concurrency)break;
      if(!this.wanted.has(key)||entry.task||entry.value.state!=='loading')continue;
      const generation=this.generation;this.active++;
      let task:Task<T>;try{task=this.load(key);}catch(e){task={promise:Promise.reject(e),cancel(){}};}
      entry.task=task;
      void task.promise.then(value=>{if(!entry.canceled&&generation===this.generation&&this.wanted.has(key))entry.value={state:'ready',value};},error=>{if(!entry.canceled&&generation===this.generation&&this.wanted.has(key))entry.value={state:'unavailable',message:error&&typeof error==='object'&&'message'in error?String(error.message):String(error)};}).finally(()=>{
        this.active--;entry.task=undefined;
        if(entry.canceled||generation!==this.generation||entry.value.state==='loading'){this.entries.delete(key);if(this.wanted.has(key))this.entries.set(key,{value:{state:'loading'},used:++this.clock});}
        this.prune();this.changed();this.pump();
      });
    }
  }
  diagnostics(){return {active:this.active,entries:this.entries.size,visible:this.wanted.size};}
}
