import {createObservation,ObservationSampler,type ObservationTransport,type ObservationClient} from '@sodeck/sdk/observation';
export const observation=createObservation((globalThis as unknown as {sodeck:ObservationTransport}).sodeck);
export const sampler=new ObservationSampler(observation);
const states=['connecting','waitingAuth','running','active','connected','failed','canceled','ended','closed','disconnected'];
export const safeState=(state:string)=>states.includes(state)?state:'unknown';
export function errorCode(error:unknown):string{const code=(error as {code?:unknown})?.code;return typeof code==='string'&&/^[A-Z][A-Z0-9_]{0,63}$/.test(code)?code:'UNKNOWN';}
type Details={attemptId?:string;resourceRefs?:string[];integrationState?:string;directoryRevision?:string;recentErrorCode?:string};
const details=new Map<string,Details>();
export function updateSessionDiagnostic(id:string,patch:Details){if(details.has(id)||details.size<32)details.set(id,{...details.get(id),...patch});}
export function releaseSessionDiagnostic(id:string){details.delete(id);}
type Session={sessionId:string;workspaceId:string;status:string};
export function stateChanged(session:Session,from:string,basis:'worker-observed'|'core-response'|'ui-reported',client:ObservationClient=observation){if(from!==session.status)void client.emit('plugin.sodeck.terminal.session.state.changed',{sessionId:session.sessionId,from:safeState(from),to:safeState(session.status),observationBasis:basis,...(details.get(session.sessionId)?.attemptId?{attemptId:details.get(session.sessionId)!.attemptId}:{})});}
const priorScopes=new Set<string>();
export function reportSessions(sessions:Iterable<Session>,pending:Iterable<{workspaceId:string}>,enabled:boolean){
  const attempts=[...pending],all=[...sessions],scopes=[...new Set([...priorScopes,...all.map(s=>s.workspaceId)])].slice(0,16);priorScopes.clear();for(const session of all)priorScopes.add(session.workspaceId);
  for(const scope of scopes){const rows=all.filter(s=>s.workspaceId===scope),counts:Record<string,number>={};for(const session of rows){const state=safeState(session.status);counts[state]=(counts[state]??0)+1;}
    sampler.snapshot('terminal.sessions.v1',scope,{total:rows.length,truncated:rows.length>32,pendingAttempts:attempts.filter(p=>p.workspaceId===scope).length,integrationEnabled:enabled,stateCounts:counts,sessions:rows.slice(0,32).map(s=>({sessionId:s.sessionId,state:safeState(s.status),...details.get(s.sessionId)}))});
    for(const state of [...states,'unknown'])sampler.metric('session.active',scope,'gauge','count',counts[state]??0,{state});
  }
}
