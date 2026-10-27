import {describe,it,expect} from 'vitest';
import cases from '../../../contracts/remote-read-v1.examples.json';
import {validateRemoteRead} from './remote-read';
const examples=cases as Array<{name:string;kind:'x-methods'|'x-results';method:string;valid:boolean;value:unknown}>;
describe('remote read shared contract examples',()=>{for(const e of examples)it(e.name,()=>expect(validateRemoteRead(e.kind,e.method,e.value)===undefined).toBe(e.valid));});
