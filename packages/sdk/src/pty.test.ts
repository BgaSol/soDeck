import {it,expect} from 'vitest';
import {openPtyCommand} from './pty';
it('queries an ambiguous start without ever replaying the command',async()=>{
 const calls:string[]=[];const pty={ptyId:'p',streamId:'p',connectionId:'c',generation:1};
 const bridge={async call<T>(method:string){calls.push(method);if(method==='pty.openCommand')throw {code:'RESULT_UNKNOWN'};return {state:'completed',result:pty,error:null} as T;}};
 expect(await openPtyCommand(bridge,{leaseId:'l',cols:80,rows:24,operationId:'o',command:'bash',observe:true})).toEqual(pty);
 expect(calls).toEqual(['pty.openCommand','pty.start.get']);
});
