import assert from 'node:assert/strict';
import {test} from 'node:test';
import {parseReading,checkpointFrames,createCheckpointReader} from '../plugins/terminal/ui-src/reading/protocol.js';
import {createReadingModel} from '../plugins/terminal/ui-src/reading/model.js';

test('reading protocol validates token, fields, sequence and unknown results',()=>{
  const prefix='SODECK;secret;reading;1;';
  assert.deepEqual(parseReading(prefix+'start;2','secret'),{kind:'start',sequence:2,exitCode:undefined});
  assert.equal(parseReading(prefix+'end;2;?','secret').exitCode,undefined);
  assert.equal(parseReading(prefix+'end;2;137','secret').exitCode,137);
  for(const text of [prefix+'start;2;extra',prefix+'end;2;-1',prefix+'end;2;NaN',prefix+'input;12\n',prefix+'prompt;1'.repeat(200)])assert.equal(parseReading(text,'secret'),undefined,text);
  assert.equal(parseReading(prefix+'start;2','other'),undefined);
  assert.equal(parseReading('SODECK;secret;ready;abc;def','secret'),undefined);
});
test('checkpoint chunks round trip Unicode and reject corrupt state',()=>{
  const data={version:1,model:{entries:Array.from({length:700},(_,id)=>({id,title:'中文🙂 cat app.log'}))}};
  let restored;const read=createCheckpointReader(value=>{restored=value;});
  for(const frame of checkpointFrames(data))read(frame.slice(6,-1));
  assert.deepEqual(restored,data);
  read('SODECK-READING;1;begin');read('SODECK-READING;1;part;7b');read('SODECK-READING;1;end');assert.equal(restored,null);
});
function fixture(){
  let cursor=0,column=2,type='normal';const lines=['$ cat app.log','line 1','line 2',''];
  const markers=[];
  const normal={baseY:0,get cursorY(){return cursor;},get cursorX(){return column;},get length(){return lines.length;},getLine:y=>({isWrapped:false,translateToString:(_trim,x=0)=>(lines[y]||'').slice(x)})};
  const terminal={cols:80,buffer:{normal,get active(){return {...normal,type};}}};
  const buffers={marker(row){if(!Number.isInteger(row)||row<0||row>=lines.length)return;const marker={line:row,isDisposed:false,dispose(){this.isDisposed=true;}};markers.push(marker);return marker;}};
  const model=createReadingModel(terminal,buffers,()=>{});
  return {model,markers,lines,cursor(value,col=2){cursor=value;column=col;},type(value){type=value;}};
}
test('command lifecycle uses displayed input; markers never invent text or results',()=>{
  const f=fixture();f.model.event({kind:'prompt',sequence:1});f.model.event({kind:'input',sequence:1});f.cursor(1);f.model.event({kind:'start',sequence:1});
  assert.equal(f.model.state().entries[0].title,'cat app.log');
  f.cursor(3);f.model.event({kind:'end',sequence:1,exitCode:2});assert.equal(f.model.state().entries[0].exitCode,2);
  f.model.add('manual',1);f.model.add('manual',1);f.model.add('input',1);assert.equal(f.model.positions().length,2);
  assert.equal(f.model.state().entries.at(-1).title,undefined);
  const saved=f.model.snapshot();f.model.restore(saved);assert.equal(f.model.positions().length,2);
  f.model.event({kind:'prompt',sequence:2});f.model.event({kind:'start',sequence:2});f.model.interrupt();assert.equal(f.model.state().entries.at(-1).status,'unknown');
});
test('alternate buffer, unmatched end and old prompt cannot create commands',()=>{
  const f=fixture();f.model.event({kind:'end',sequence:1,exitCode:0});assert.equal(f.model.positions().length,0);
  f.model.event({kind:'prompt',sequence:9});f.model.event({kind:'prompt',sequence:1});f.model.event({kind:'start',sequence:1});assert.equal(f.model.positions().length,0);
  f.type('alternate');f.model.add('input',1);f.model.event({kind:'start',sequence:9});assert.equal(f.model.positions().length,0);
  f.type('normal');f.model.add('manual',1);f.model.state().entries[0].start.dispose();assert.equal(f.model.positions().length,0);
});
