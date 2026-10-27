import {describe,it,expect} from 'vitest';
import {validateConnectionRoute,decodeRouteStatus,decodeRouteStatusV2} from './connection-route';
describe('additive connection route contract',()=>{
  const server={name:'目标',host:'target.internal',port:22,username:'user',authType:'password',connectionRoute:{kind:'sshJump',jumpServerId:'gateway'}};
  it('accepts explicit single hop and rejects silently ignored paths',()=>{
    expect(validateConnectionRoute('x-methods','servers.routes.create',server)).toBeUndefined();
    for(const route of [{kind:'socks'},{kind:'sshJump'},{kind:'direct',jumpServerId:'stale'},{kind:'sshJump',jumpServerId:'a',hops:['b']}]){
      expect(validateConnectionRoute('x-methods','servers.routes.create',{...server,connectionRoute:route})).toBeTruthy();
    }
  });
  it('rejects credentials and invalid node roles in public route snapshots',()=>{
    const node={serverId:'id',name:'name',host:'host',port:22,username:'user',authType:'key',configRevision:1,role:'target'};
    const status={saved:{targetServerId:'id',routeRevision:'rev',nodes:[node]},running:[],attempt:null};
    expect(decodeRouteStatus(status)).toEqual(status);
    expect(()=>decodeRouteStatus({...status,saved:{...status.saved,password:'secret'}})).toThrow();
    expect(()=>decodeRouteStatus({...status,saved:{...status.saved,nodes:[{...node,role:'proxy'}]}})).toThrow();
    expect(()=>decodeRouteStatus({...status,saved:{...status.saved,nodes:[]}})).toThrow();
  });
});

describe('multihop route v2 remains additive and bounded',()=>{
  const reference={kind:'saved',serverId:'target',routeRevision:'r1'};
  const route={targetServerId:'target',targetName:'Target',routeRevision:'r1',nodeCount:1001,reference};
  const node={serverId:'target',name:'Target',host:'private',port:22,username:'user',authType:'password',configRevision:1,role:'target'};
  const status={saved:route,running:[],runningCount:0,currentCount:0,changedCount:0,attempt:{route,hopIndex:1000,current:node,stage:'authenticate',error:null,updated:1}};
  it('accepts deep metadata and safe indices without widening v1',()=>{
    expect(decodeRouteStatusV2(status)).toEqual(status);
    expect(()=>decodeRouteStatusV2({...status,attempt:{...status.attempt,hopIndex:1001}})).toThrow();
    expect(()=>decodeRouteStatusV2({...status,saved:{...route,reference:{...reference,routeRevision:'wrong'}}})).toThrow();
    const old={saved:{targetServerId:'target',routeRevision:'r1',nodes:[node,node,node]},running:[],attempt:null};
    expect(()=>decodeRouteStatus(old)).toThrow();
  });
  it('bounds each page independently of total path length and excludes secrets',()=>{
    const page={route,nodes:Array.from({length:64},()=>node),offset:0,nextCursor:'cursor'};
    expect(validateConnectionRoute('x-results','connections.route.v2.nodes',page)).toBeUndefined();
    expect(validateConnectionRoute('x-results','connections.route.v2.nodes',{...page,nodes:[...page.nodes,node]})).toBeTruthy();
    expect(validateConnectionRoute('x-results','connections.route.v2.nodes',{...page,nodes:[{...node,password:'secret'}]})).toBeTruthy();
    expect(validateConnectionRoute('x-methods','connections.route.v2.nodes',{reference:{...reference,connectionId:'wrong-scope'}})).toBeTruthy();
  });
});
