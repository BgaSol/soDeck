import {describe,it,expect} from 'vitest';
import {parseCpu,cpuDifference,parseMemory,parseDisk,parseNet,parseMounts,parseCapacity} from './parse';
import {Counters} from './counters';
import {History} from './history';
import {Model} from '../model';
import {capacityScript} from '../collectors/scripts';
import {byteText,parseSections,rawBytes} from '../collectors/framing';
describe('Linux metrics and identities',()=>{
  it('excludes guest double counting and iowait from busy',()=>{
    const a=parseCpu('cpu 100 0 0 100 100 0 0 0 50 0');
    const b=parseCpu('cpu 120 0 0 120 160 0 0 0 70 0');
    expect(cpuDifference(b,a)).toEqual({percent:20,iowait:60});
    expect(cpuDifference(a,b).percent).toBeNull();
  });
  it('uses binary kB and marks compatibility memory estimates',()=>{
    const m=parseMemory('MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 100 kB\nCached: 200 kB\nSReclaimable: 100 kB\nShmem: 50 kB\nSwapTotal: 200 kB\nSwapFree: 50 kB');
    expect(m).toMatchObject({total:'1024000',available:'460800',used:'563200',percent:55,estimated:true,swapUsed:'153600'});
    expect(()=>parseMemory('MemTotal: -1 kB\nMemFree: 0 kB')).toThrow();
  });
  it('uses disk sectors of 512 bytes and unsigned byte counters',()=>{
    expect(parseDisk('8 0 sda 1 0 3 0 1 0 5 0 0 0 0')[0]).toMatchObject({first:'1536',second:'2560'});
    expect(parseNet('eth0: 9007199254740993 0 0 0 0 0 0 0 20 0 0 0 0 0 0 0')[0].first).toBe('9007199254740993');
  });
  it('breaks baselines on identity, clock source, counter rollback, or long gaps',()=>{
    const c=new Counters(),row={id:'1:eth0',name:'eth0',first:'9007199254740993',second:'10'};
    expect(c.update([row],1000,'remote',10000)[0].first).toBeNull();
    expect(c.update([{...row,first:'9007199254742993',second:'20'}],3000,'remote',10000)[0].first).toBe(1000);
    expect(c.update([row],4000,'local',10000)[0].first).toBeNull();
    expect(c.update([row],20000,'local',10000)[0].first).toBeNull();
    c.update([],21000,'local',10000);expect(c.update([row],22000,'local',10000)[0].first).toBeNull();
  });
  it('retains raw escaped paths, stat reserved blocks and mount identity',()=>{
    const line="20 1 8:0 / /a\\040b\\012c'$(touch\\040BAD) rw - ext4 /dev/sda rw";
    const mount=parseMounts(line)[0];expect(mount.rawPath).toBe("/a b\nc'$(touch BAD)");
    const script=byteText(capacityScript('token',mount));expect(script).toContain("'\\''");
    const sections={values:{before:line,after:line,capacity:'STAT 4096 100 40 30'},errors:{},partial:false};
    expect(parseCapacity(sections,mount)).toMatchObject({total:'409600',used:'245760',available:'122880',percent:66.6});
    expect(()=>parseCapacity({...sections,values:{...sections.values,after:line.replace('20 1','21 1')}},mount)).toThrow();
  });
  it('rejects incomplete, oversized, repeated sections while retaining valid groups',()=>{
    const result=parseSections(rawBytes('noise\nSODECK_MON_V1 t cpu BEGIN\n123\nSODECK_MON_V1 t cpu END 0\nSODECK_MON_V1 t memory BEGIN\nbroken'),'t',{cpu:10,memory:10});
    expect(result.values.cpu).toBe('123');expect(result.partial).toBe(true);expect(result.values.memory).toBeUndefined();
  });
  it('keeps ten minutes under repeated refresh, with gaps and bounded memory',()=>{
    const h=new History();for(let t=0;t<=1200000;t+=100)h.add('cpu',t,50);
    const p=h.read('cpu',1200000,600);expect(p.length).toBeLessThanOrEqual(301);expect(p[0].time).toBeLessThan(-598000);
    h.gap(1202000);expect(h.read('cpu',1202000,600).at(-1)?.value).toBeNull();expect(h.bytes()).toBeLessThan(11000);
  });
  it('does not erase a complete inventory after a truncated update',()=>{
    const m=new Model(),inventory={os:'Linux',host:'one',kernel:'k',boot:'b',cores:2,mounts:parseMounts('1 0 8:0 / / rw - ext4 /dev/sda rw'),interfaces:{},devices:{},partial:false};
    m.installInventory(inventory,0);m.installInventory({...inventory,mounts:[],partial:true},2000);
    expect(m.inventory?.mounts).toHaveLength(1);expect(m.partial).toBe(true);
  });
  it('rejects both slow local endpoints and rewarms after clock-source changes',()=>{
    const m=new Model(),data={cpu:[1n,0n,0n,1n,0n,0n,0n,0n],memory:null,load:[],uptime:null,net:[{id:'eth',name:'eth',first:'100',second:'100'}],disk:[],errors:{},partial:false};
    m.installFast(data,0,3000,3000);m.installFast({...data,cpu:[2n,0n,0n,2n,0n,0n,0n,0n]},4000,4500,4500);expect(m.cpu.value?.percent).toBeNull();expect(m.network.value?.[0].first).toBeNull();
    m.installFast({...data,cpu:[3n,0n,0n,3n,0n,0n,0n,0n]},6000,6500,6500);expect(m.cpu.value?.percent).toBe(50);
    m.installFast({...data,uptime:10},8000,8500,8500);expect(m.cpu.value?.percent).toBeNull();
  });
});
