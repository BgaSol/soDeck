// Summarize completed evidence; never turn a partial/failed run into a pass.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {resolve,join,sep} from 'node:path';
const root=resolve('.qa/t024/load'),data=resolve(process.argv[2]??'');
assert.ok(data.startsWith(root+sep),'Pass the exact isolated load data directory');
const result=JSON.parse(await readFile(join(root,'results.json'))),samples=result.samples;
const fixtureBaseline=JSON.parse(await readFile(join(root,'fixture-baseline.json')));
const processShutdown=JSON.parse(await readFile(join(data,'shutdown.json')));
assert.equal(fixtureBaseline.start,result.start);assert.equal(resolve(fixtureBaseline.dataDirectory),data);
assert.equal(result.failure,undefined);assert.ok(result.elapsedMs>=1800000);
assert.ok(result.checks.includes('all test leases returned to zero'));
const percentile=(values,p)=>{const sorted=values.filter(Number.isFinite).sort((a,b)=>a-b);return sorted[Math.min(sorted.length-1,Math.ceil(sorted.length*p)-1)]??null;};
const distribution=values=>({p50:percentile(values,.5),p95:percentile(values,.95),maximum:Math.max(...values)});
const peak=fn=>Math.max(...samples.map(fn));
const python=process.env.SODECK_TEST_PYTHON;assert.ok(python,'Set SODECK_TEST_PYTHON');
const audit=JSON.parse(execFileSync(python,['-c',String.raw`
import sqlite3,json,sys
from pathlib import Path
root=Path(sys.argv[1])/'observability'
facts=sqlite3.connect((root/'facts.sqlite').as_uri()+'?mode=ro',uri=True)
diagnostics=sqlite3.connect((root/'diagnostics.sqlite').as_uri()+'?mode=ro',uri=True)
rows=[json.loads(r[0]) for r in diagnostics.execute("select data from metric_buckets where plugin_id='sodeck.monitor' and type='monitor.collection.duration'")]
errors=[{'code':r[0],'count':r[1]} for r in facts.execute("select error_code,count(*) from calls where error_code is not null group by error_code")]
print(json.dumps({'retainedCalls':facts.execute('select count(*) from calls').fetchone()[0],'unfinishedRetainedCalls':facts.execute('select count(*) from calls where ended is null').fetchone()[0],'recordedCallErrors':errors,'collectionBuckets':rows}))
`,data],{windowsHide:true,encoding:'utf8'}));
const histograms={};
for(const row of audit.collectionBuckets){if(row.fromUtc<result.start||row.toUtc>result.start+result.elapsedMs)continue;const lane=row.labels.lane,entry=histograms[lane]??={count:0,boundariesMs:row.definition.buckets,buckets:Array(row.buckets.length).fill(0)};entry.count+=row.count;row.buckets.forEach((v,i)=>entry.buckets[i]+=v);}
for(const entry of Object.values(histograms)){let cumulative=0;entry.p95UpperBoundMs=null;for(let i=0;i<entry.buckets.length;i++){cumulative+=entry.buckets[i];if(cumulative>=entry.count*.95){entry.p95UpperBoundMs=entry.boundariesMs[i]??null;break;}}}
delete audit.collectionBuckets;
const load=JSON.parse(await readFile(join(root,'controlled-load-guest.json'))),idle=JSON.parse(await readFile(join(root,'idle-after-load-guest.json')));
assert.equal(load.loadStart,result.start);assert.ok(load.measuredAt>=result.start&&load.measuredAt<result.start+result.elapsedMs);
assert.equal(idle.loadStart,result.start);assert.ok(idle.measuredAt>=result.start+result.elapsedMs);
const targets=Array.from({length:8},(_,i)=>({target:i+1,distinctSampleTimes:new Set(samples.map(s=>s.targets[i].cpuAt)).size,sampleAgeMs:distribution(samples.map(s=>result.start+s.elapsedMs-s.targets[i].cpuAt)),peakCacheBytes:peak(s=>s.targets[i].cache),freshObservations:samples.filter(s=>s.targets[i].quality==='fresh').length,observations:samples.length}));
const summary={passed:true,start:result.start,elapsedMs:result.elapsedMs,iterations:samples.length,environment:result.environment,fixture:'Eight controlled 64-NIC / 64-disk / 128-mount targets through real SSH, native QuickJS worker; one real terminal, file browser and 1 MiB tunnel transfer loop',peaks:{controllers:peak(s=>s.controllers),targets:peak(s=>s.scheduler.targets),fast:peak(s=>s.scheduler.fast),slow:peak(s=>s.scheduler.slow),transientBytes:peak(s=>s.scheduler.transientBytes),pendingRpc:peak(s=>s.rpc.pending),ordinaryRpc:peak(s=>s.rpc.ordinary),cacheBytes:peak(s=>s.cacheBytes),workerRssBytes:peak(s=>s.workerRss??0)},terminalEchoMs:distribution(samples.map(s=>s.echoMs)),transferAndObservationMs:distribution(samples.map(s=>s.transferMs)),fileRefreshUnknownCount:samples.filter(s=>s.refreshUnknown).length,targets,collectionHistograms:histograms,storage:{last:samples.filter(s=>s.storage).at(-1)?.storage,audit},guestMeasurements:{load,idle},limitations:['Peak counts are sampled alongside worker hard admission limits, not a continuous profiler.','transferAndObservationMs includes the HTTP transfer and subsequent diagnostic/snapshot measurement overhead.','Guest CPU/network observations cover the entire isolated VM including all test traffic; they do not attribute overhead to this plugin.','Collection p95 is a histogram bucket bound, not an exact percentile.']};
summary.fixtureBaseline=fixtureBaseline;
summary.processShutdown=processShutdown;
await writeFile(join(root,'summary.json'),JSON.stringify(summary,null,2));console.log(JSON.stringify({elapsedMs:summary.elapsedMs,iterations:summary.iterations,peaks:summary.peaks,terminalEchoMs:summary.terminalEchoMs,collectionHistograms:histograms},null,2));
