import {expect,it} from 'vitest';
import {validatePluginManagement as check} from './plugin-management';
it('strictly checks management payloads, statuses and capacities without TextEncoder',()=>{
  const p={generation:crypto.randomUUID(),expectedRevision:'0',operationId:crypto.randomUUID(),changes:[{pluginId:'a',enabled:false}]};
  expect(check('x-methods','plugins.enablement.commit',p)).toBeUndefined();
  expect(check('x-methods','plugins.enablement.commit',{...p,changes:[{pluginId:'a',enabled:'false'}]})).toBeDefined();
  expect(check('x-methods','plugins.management.list',{limit:101})).toBeDefined();
  expect(check('x-methods','plugins.enablement.commit',{...p,extra:true})).toBeDefined();
  expect(check('x-results','plugins.package.prepare',{operationId:p.operationId,state:'prepared',signature:{status:'unverified'}})).toBeUndefined();
  expect(check('x-results','plugins.package.prepare',{operationId:p.operationId,state:'prepared',signature:{status:'trusted'}})).toBeDefined();
  expect(check('x-methods','plugins.enablement.commit',{...p,changes:Array.from({length:1025},()=>p.changes[0])})).toBeDefined();
});
