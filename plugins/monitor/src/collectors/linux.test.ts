import {it,expect} from 'vitest';
import {LinuxCollector} from './linux';
import type {BoundedExecClient} from '@sodeck/sdk/bounded-exec';
import {byteText,rawBytes} from './framing';
it('caches an unavailable group without delaying healthy fast groups',async()=>{
 const scripts:string[]=[];
 const collector=new LinuxCollector({run:async(request:{stdin:Uint8Array;operationId:string})=>{
  const script=byteText(request.stdin);scripts.push(script);let output='';
  for(const [name,value]of Object.entries({before:'10 0',cpu:'cpu 1 0 0 1',memory:'',load:'0 0 0',net:'lo: 1 0 0 0 0 0 0 0 1 0 0 0 0 0 0 0',disk:'8 0 sda 1 0 1 0 1 0 1 0 0 0 0',after:'10 0'})){
   if(name==='memory'&&!script.includes('part memory'))continue;
   output+=`SODECK_MON_V1 ${request.operationId} ${name} BEGIN\n${value}\n\nSODECK_MON_V1 ${request.operationId} ${name} END ${name==='memory'?1:0}\n`;
  }
  return {stdout:rawBytes(output),stderr:new Uint8Array(),exitCode:0,endedReason:null,truncated:false,elapsedMs:1,cleanupPending:false};
 }} as unknown as BoundedExecClient);
 const target={scope:{workspaceId:'w',workspaceGeneration:'g'},leaseId:'l',connectionGeneration:1};
 await collector.fast(target,'one',undefined,0);const next=await collector.fast(target,'two',undefined,2000);expect(scripts[1]).not.toContain('part memory');expect(scripts[1]).toContain('begin cpu');expect(next.cpu).not.toBeNull();expect(next.partial).toBe(true);
 await collector.fast(target,'three',undefined,60001);expect(scripts[2]).not.toContain('part memory');collector.reset();await collector.fast(target,'four',undefined,62000);expect(scripts[3]).toContain('part memory');
});
