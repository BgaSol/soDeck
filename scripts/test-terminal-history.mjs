import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {chromium} from 'playwright';

const browser=await chromium.launch({headless:true});
try {
  const page=await browser.newPage();
  await page.setContent('<!doctype html><meta charset="utf-8">');
  for(const file of ['@xterm/xterm/lib/xterm.js','@xterm/addon-serialize/lib/addon-serialize.js'])await page.addScriptTag({path:path.resolve('plugins/terminal/node_modules',file)});
  await page.addScriptTag({content:fs.readFileSync('plugins/terminal/ui-src/snapshot.js','utf8').replaceAll('export ','')});
  const result=await page.evaluate(async()=>{
    const terminal=new Terminal({allowProposedApi:true,cols:120,rows:24,scrollback:100000});
    const addon=new SerializeAddon.SerializeAddon();terminal.loadAddon(addon);
    const write=(term,text)=>new Promise(resolve=>term.write(text,resolve));
    const start=performance.now();
    for(let begin=0;begin<100000;begin+=1000)await write(terminal,Array.from({length:1000},(_,i)=>`\x1b[${(begin+i)%7+31}m${String(begin+i).padStart(6,'0')} timeout 中文🙂 ${'x'.repeat((begin+i)%135)}\x1b[0m\r\n`).join(''));
    const parseMs=performance.now()-start;
    let now=performance.now();const full=addon.serialize();const fullSerializeMs=performance.now()-now;
    const restored=new Terminal({allowProposedApi:true,cols:120,rows:24,scrollback:100000});
    const length=terminal.buffer.normal.length,pieces=[];let maxSliceMs=0;
    const hash=(term,buffer=term.buffer.normal)=>{const rows=new Uint32Array(buffer.length),cell=buffer.getNullCell();for(let y=0;y<rows.length;y++){const line=buffer.getLine(y);let value=line.isWrapped?7:3;for(let x=0;x<line.length;x++){line.getCell(x,cell);const text=cell.getChars();for(let k=0;k<text.length;k++)value=Math.imul(value^text.charCodeAt(k),16777619);for(const method of ['getWidth','getFgColor','getBgColor','getFgColorMode','getBgColorMode','isBold','isDim','isBlink','isItalic','isUnderline','isInverse','isInvisible','isStrikethrough','isOverline'])value=Math.imul(value^cell[method](),16777619);}rows[y]=value;}return rows;};
    await write(restored,full);const expected=hash(restored),cursor=[restored.buffer.normal.cursorX,restored.buffer.normal.cursorY];restored.reset();
    const iterator=snapshotChunks(terminal,addon),captureStart=performance.now();let iteration=0;
    while(true){now=performance.now();const result=await iterator.next();maxSliceMs=Math.max(maxSliceMs,performance.now()-now);if(result.done)break;pieces.push(result.value);if(iteration++%8===0)await write(terminal,('\x1b[38;2;80;120;180mcontinued output 中文🙂\x1b[0m\r\n').repeat(800));}
    const snapshotMs=performance.now()-captureStart;
    for(const piece of pieces)await write(restored,piece);
    let mismatch;const actual=hash(restored);
    if(restored.buffer.normal.length!==length)mismatch={length,restored:restored.buffer.normal.length};
    for(let row=0;!mismatch&&row<length;row++)if(expected[row]!==actual[row])mismatch={row,expected:expected[row],actual:actual[row]};
    const report={parseMs,fullSerializeMs,maxSliceMs,snapshotMs,concurrentOutputRows:Math.ceil(iteration/8)*800,snapshotBytes:new TextEncoder().encode(full).length,rows:length,fragmentBytes:pieces.reduce((n,p)=>n+new TextEncoder().encode(p).length,0),mismatch,cursor:[...cursor,restored.buffer.normal.cursorX,restored.buffer.normal.cursorY]};
    terminal.dispose();restored.dispose();
    const cases=[];
    for(const alternate of [false,true]){
      const source=new Terminal({allowProposedApi:true,cols:23,rows:7,scrollback:100000}),baseline=new Terminal({allowProposedApi:true,cols:23,rows:7,scrollback:100000}),copy=new Terminal({allowProposedApi:true,cols:23,rows:7,scrollback:100000}),serializer=new SerializeAddon.SerializeAddon();source.loadAddon(serializer);
      await write(source,Array.from({length:160},(_,n)=>`\x1b[0;1;3;4;7;9m${n} 中文🙂\x1b[0;48;2;20;100;150m   \x1b[K\x1b[0m${'z'.repeat(n%32)}\r\n`).join('')+'\x1b[?1h\x1b[?2004h\x1b[2;4H\x1b[38;2;55;66;77;48;5;234;2;53mcurrent');
      if(alternate)await write(source,'\x1b[?1049h\x1b[3;5H\x1b[1;4;38;5;170malternate 中文🙂');
      await write(baseline,serializer.serialize());for await(const part of snapshotChunks(source,serializer))await write(copy,part);
      const equal=(a,b)=>a.length===b.length&&a.every((n,i)=>n===b[i]);const normal=equal(hash(baseline),hash(copy)),alt=equal(hash(baseline,baseline.buffer.alternate),hash(copy,copy.buffer.alternate));
      cases.push({alternate,normal,alt,modes:JSON.stringify(baseline.modes)===JSON.stringify(copy.modes),cursor:baseline.buffer.active.cursorX===copy.buffer.active.cursorX&&baseline.buffer.active.cursorY===copy.buffer.active.cursorY,active:baseline.buffer.active.type===copy.buffer.active.type});source.dispose();baseline.dispose();copy.dispose();
    }
    report.styleCases=cases;return report;
  });
  fs.mkdirSync('artifacts/terminal-history',{recursive:true});fs.writeFileSync('artifacts/terminal-history/serialization.json',JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify(result,null,2));assert.equal(result.mismatch,undefined);assert.deepEqual(result.cursor.slice(0,2),result.cursor.slice(2));
  for(const sample of result.styleCases)for(const key of ['normal','alt','modes','cursor','active'])assert.equal(sample[key],true,JSON.stringify(sample));
}finally{await browser.close();}
