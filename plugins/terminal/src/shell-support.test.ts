import {it,expect} from 'vitest';
import {Cancellation,type BoundedExecClient,type ExecResult} from '@sodeck/sdk/bounded-exec';
import {utf8} from '@sodeck/sdk/bytes';
import {supportsShellIntegration,shellProbeCommand} from './shell-support';

it('requires a complete successful Linux+Bash probe and uses bounded read-only execution',async()=>{
  const base:ExecResult={stdout:utf8('SODECK_SHELL:5.2.37(1)-release\nLinux\n'),stderr:new Uint8Array(),exitCode:0,endedReason:null,truncated:false,cleanupPending:false,elapsedMs:5};
  const target={scope:{workspaceId:'w',workspaceGeneration:'g'},leaseId:'lease',connectionGeneration:5,operationId:'probe'};
  for(const patch of [{},{exitCode:1},{endedReason:'TIMEOUT'},{truncated:true},{cleanupPending:true},{stdout:utf8('SODECK_SHELL:\nLinux\n')}]){
    const client={run:async(request:unknown)=>{expect(request).toMatchObject({...target,command:shellProbeCommand,remoteTimeoutMs:2000,deadlineMs:3000,maxOutputBytes:1024});return {...base,...patch};}} as BoundedExecClient;
    expect(await supportsShellIntegration(client,target,new Cancellation())).toBe(Object.keys(patch).length===0);
  }
});
