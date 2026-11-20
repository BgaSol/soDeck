export const READING_VERSION=1;
export function parseReading(text,token){
  if(!token||text.length>1024)return;
  const p=text.split(';');
  if(p[0]!=='SODECK'||p[1]!==token)return;
  if(p[2]==='unavailable')return {kind:'unavailable'};
  if(p[2]!=='reading'||p[3]!=='1'||!['prompt','input','start','end'].includes(p[4])||!/^\d{1,12}$/.test(p[5]||''))return;
  if(p.length!==(p[4]==='end'?7:6))return;
  if(p[4]==='end'&&!/^(?:\?|[0-9]{1,10})$/.test(p[6]))return;
  return {kind:p[4],sequence:Number(p[5]),exitCode:p[6]!==undefined&&p[6]!=='?'?Number(p[6]):undefined};
}
export function checkpointFrames(snapshot){
  // Stored only in plugin-generated display checkpoints, never accepted from live PTY output.
  const bytes=new TextEncoder().encode(JSON.stringify(snapshot));
  if(bytes.length>16*1024*1024)throw new Error('阅读标记超过恢复容量，当前页面仍可使用');
  const frames=['\x1b]633;SODECK-READING;1;begin\x07'];
  for(let pos=0;pos<bytes.length;pos+=4096){let hex='';for(const b of bytes.subarray(pos,pos+4096))hex+=b.toString(16).padStart(2,'0');frames.push('\x1b]633;SODECK-READING;1;part;'+hex+'\x07');}
  frames.push('\x1b]633;SODECK-READING;1;end\x07');return frames;
}
export function createCheckpointReader(restore){
  let bytes=[],size=0,active=false;
  return text=>{
    if(!text.startsWith('SODECK-READING;1;'))return false;
    const p=text.split(';');
    if(p[2]==='begin'){bytes=[];size=0;active=true;}
    else if(p[2]==='part'&&active&&p.length===4&&p[3].length<=8192&&/^(?:[0-9a-f]{2})+$/.test(p[3])){
      size+=p[3].length/2;if(size>16*1024*1024){active=false;bytes=[];}else bytes.push(Uint8Array.from(p[3].match(/../g),v=>parseInt(v,16)));
    }else if(p[2]==='end'&&active){active=false;const all=new Uint8Array(size);let pos=0;for(const part of bytes){all.set(part,pos);pos+=part.length;}bytes=[];try{restore(JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(all)));}catch{restore(null);}}
    else {active=false;bytes=[];}
    return true;
  };
}
