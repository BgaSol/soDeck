import {createObservation,ObservationSampler,DiagnosticEntities,type ObservationClient,type ObservationTransport} from '@sodeck/sdk/observation';
export let observation:ObservationClient|undefined;
export let sampler:ObservationSampler|undefined;
export function configureObservation(bridge:ObservationTransport){observation=createObservation(bridge);sampler=new ObservationSampler(observation);}
export function errorCode(error:unknown):string{const code=(error as {code?:unknown})?.code;return typeof code==='string'&&/^[A-Z][A-Z0-9_]{0,63}$/.test(code)?code:'UNKNOWN';}
export {DiagnosticEntities};
export type {ObservationClient};
