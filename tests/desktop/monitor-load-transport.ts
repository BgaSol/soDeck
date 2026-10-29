// Test package only: controlled maximum object counts cross the real SSH exec
// channel. This is deliberately excluded from the production plugin package.
import {encode,decode,utf8} from '../../packages/sdk/src/bytes';
import type {WorkerTransport} from '../../packages/sdk/src/worker';
export function fixtureTransport(base:WorkerTransport):WorkerTransport {
  let firstScope='',failedScope='',failedUntil=0;
  (globalThis as any).__monitorLoadFault=(scope:string)=>{failedScope=scope;failedUntil=Date.now()+10000;};
  const mount=(n:number)=>`${n+1} 0 8:${n} / ${n===0?'/':'/controlled/'+n} rw - ext4 /dev/controlled${n} rw`;
  return {...base,async call<T>(method:string,params:any={},metadata:any):Promise<T>{
    if(params.scope?.workspaceId===failedScope&&Date.now()<failedUntil){
      if(method==='connections.observe')return {state:'disconnected',revision:1,connection:null} as T;
      if(method==='exec.start')throw Object.assign(Error('Controlled ten-second transport interruption'),{code:'CONNECTION_NOT_READY'});
    }
    if(method!=='exec.start')return base.call(method,params,metadata);
    firstScope||=params.scope.workspaceId;
    const source=String.fromCharCode(...decode(params.stdinBase64)),token=/TOKEN='([^']+)'/.exec(source)![1];
    const section=(name:string,body:string)=>`SODECK_MON_V1 ${token} ${name} BEGIN\n${body}\n\nSODECK_MON_V1 ${token} ${name} END 0\n`;
    const n=Math.floor((globalThis as any).sodeck.clock.monotonicMs()),t=n/1000+100;
    let output='';
    if(source.includes('begin os')){
      for(const [name,body]of Object.entries({os:'Linux',host:'CONTROLLED-LOAD-FIXTURE',kernel:'fixture-over-real-ssh',boot:'controlled-boot',online:'0-63',mounts:Array.from({length:128},(_,i)=>mount(i)).join('\n'),interfaces:Array.from({length:64},(_,i)=>`nic${i} ${i+1} 1`).join('\n'),devices:Array.from({length:64},(_,i)=>`disk${i} 8:${i} 0`).join('\n')}))output+=section(name,body);
    }else if(source.includes('begin capacity')){
      if(params.scope.workspaceId===firstScope)return base.call(method,{...params,stdinBase64:encode(utf8('sleep 8\n'))},metadata);
      const id=Number(/case "\$row" in '(\d+) '/.exec(source)![1])-1;
      for(const [name,body]of Object.entries({before:mount(id),capacity:'STAT 4096 1000000 400000 300000',after:mount(id)}))output+=section(name,body);
    }else{
      for(const [name,body]of Object.entries({before:`${t} 0`,cpu:`cpu ${n+100} 0 0 ${n*4+100} 0 0 0 0`,memory:'MemTotal: 67108864 kB\nMemFree: 16777216 kB\nMemAvailable: 33554432 kB',load:'0.1 0.2 0.3 1/100 1',net:Array.from({length:64},(_,i)=>`nic${i}: ${n*100+i} 0 0 0 0 0 0 0 ${n*200+i} 0 0 0 0 0 0 0`).join('\n'),disk:Array.from({length:64},(_,i)=>`8 ${i} disk${i} 1 0 ${n*2+i} 0 1 0 ${n*3+i} 0 0 0 0`).join('\n'),after:`${t} 0`}))output+=section(name,body);
    }
    const stdin=utf8("printf '%s' '"+output.replace(/'/g,"'\\''")+"'\n");
    return base.call(method,{...params,stdinBase64:encode(stdin)},metadata);
  }};
}
