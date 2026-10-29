import {describe,it,expect} from 'vitest';
import {validateAccessPath,decodeRouteStatus} from './connection-route';
const server={name:'目标',host:'target.internal',port:22,username:'user',authType:'password',accessPath:{steps:[]}};
const reference={kind:'saved',serverId:'target',routeRevision:'r1'};
const route={targetServerId:'target',targetName:'Target',routeRevision:'r1',stepCount:3,sshCount:2,proxyCount:1,attemptTimeoutMs:150000,reference};
const node={serverId:'target',name:'Target',host:'private',port:22,username:'user',authType:'password',configRevision:1,role:'target',kind:'ssh',stepId:'target',stepIndex:2,fromStepId:'proxy',hasBoundCredential:false,arrival:{edgeId:'proxy:target',fromStepId:'proxy',toStepId:'target',mechanism:'socksConnect',resolve:'proxy'}};
const status={saved:route,running:[],runningCount:0,currentCount:0,changedCount:0,attempt:{route,stepIndex:2,attemptGeneration:1,operationId:'op',current:node,stage:'authenticate',error:null,updated:1}};
describe('API 6 access paths',()=>{
  it('accepts ordered typed steps and rejects legacy shapes',()=>{
    for(const accessPath of [{steps:[]},{steps:[{kind:'ssh',stepId:'00000000-0000-4000-8000-000000000001',serverId:'a'},{kind:'socks5',stepId:'00000000-0000-4000-8000-000000000002',proxyId:'proxy',resolve:'proxy'}]}])expect(validateAccessPath('x-methods','servers.routes.create',{...server,accessPath})).toBeUndefined();
    for(const accessPath of [{kind:'sshJump',jumpServerId:'a'},{kind:'hostTunnel',jumpServerIds:[]},{kind:'networkProxy',proxyId:'proxy'},{kind:'direct',proxyId:'stale'}])expect(validateAccessPath('x-methods','servers.routes.create',{...server,accessPath})).toBeTruthy();
  });
  it('keeps SOCKS challenges free of SSH host and hop fields',()=>{
    const proxy={id:'proxy',name:'Proxy',host:'127.0.0.1',port:1080,authType:'usernamePassword',username:'proxy-user',revision:1,hasPreset:false};
    const challenge={targetServerId:'target',workspaceId:'workspace',operationId:null,route,ownerKind:'socks5',ownerId:'proxy',proxy,stepId:'proxy',stepIndex:1,stepCount:3};
    expect(validateAccessPath('x-results','auth.challenge.get',challenge)).toBeUndefined();
    for(const extra of [{server},{role:'target'},{hopIndex:0},{hopCount:1},{proxy:null},{ownerKind:'targetSsh'}])expect(validateAccessPath('x-results','auth.challenge.get',{...challenge,...extra})).toBeTruthy();
  });
  it('rejects invalid indices and stale snapshot references',()=>{
    expect(decodeRouteStatus(status)).toEqual(status);
    expect(()=>decodeRouteStatus({...status,attempt:{...status.attempt,stepIndex:3}})).toThrow();
    expect(()=>decodeRouteStatus({...status,saved:{...route,reference:{...reference,routeRevision:'wrong'}}})).toThrow();
  });
  it('bounds diagnostics and prevents proxy secrets in responses',()=>{
    const page={route,nodes:Array.from({length:64},()=>node),offset:0,nextCursor:'cursor'};
    expect(validateAccessPath('x-results','connections.route.nodes',page)).toBeUndefined();
    expect(validateAccessPath('x-results','connections.route.nodes',{...page,nodes:[...page.nodes,node]})).toBeTruthy();
    expect(validateAccessPath('x-results','connections.route.nodes',{...page,nodes:[{...node,password:'secret'}]})).toBeTruthy();
    const proxy={id:'proxy',name:'Proxy',host:'127.0.0.1',port:1080,authType:'none',username:'',revision:1,hasPreset:false};
    expect(validateAccessPath('x-results','proxies.socks5.get',proxy)).toBeUndefined();
    expect(validateAccessPath('x-results','proxies.socks5.get',{...proxy,password:'secret'})).toBeTruthy();
  });
});
