// Test-only stored-ZIP reconstruction for valid-signature malformed descriptors.
import {readFile} from 'node:fs/promises';
import {createHash,createPrivateKey,sign} from 'node:crypto';
export async function fixturePackage(path,mutate,{resign=true}={}){
  const archive=await readFile(path),entries=new Map();let offset=0;
  while(archive.readUInt32LE(offset)===0x04034b50){const size=archive.readUInt32LE(offset+18),n=archive.readUInt16LE(offset+26),extra=archive.readUInt16LE(offset+28),start=offset+30+n+extra;if(archive.readUInt16LE(offset+8)!==0)throw Error('Expected stored test ZIP');entries.set(archive.toString('utf8',offset+30,offset+30+n),Buffer.from(archive.subarray(start,start+size)));offset=start+size;}
  mutate(entries);
  if(resign){const integrity=Buffer.from(JSON.stringify(Object.fromEntries([...entries].filter(([n])=>!['integrity.json','signature.json'].includes(n)).sort(([a],[b])=>a.localeCompare(b,'en')).map(([n,b])=>[n,createHash('sha256').update(b).digest('hex')]))));const key=createPrivateKey(await readFile('.local/signing/development-private.pem'));entries.set('integrity.json',integrity);entries.set('signature.json',Buffer.from(JSON.stringify({keyId:'sodeck-local-development',signature:sign(null,integrity,key).toString('base64')})));}
  const table=Array.from({length:256},(_,i)=>{for(let n=0;n<8;n++)i=i&1?(i>>>1)^0xedb88320:i>>>1;return i>>>0;});
  const local=[],central=[];offset=0;
  for(const [name,bytes] of entries){const n=Buffer.from(name);let c=0xffffffff;for(const b of bytes)c=table[(c^b)&255]^(c>>>8);const crc=(c^0xffffffff)>>>0;
    const h=Buffer.alloc(30);h.writeUInt32LE(0x04034b50);h.writeUInt16LE(20,4);h.writeUInt16LE(0x800,6);h.writeUInt32LE(crc,14);h.writeUInt32LE(bytes.length,18);h.writeUInt32LE(bytes.length,22);h.writeUInt16LE(n.length,26);local.push(h,n,bytes);
    const d=Buffer.alloc(46);d.writeUInt32LE(0x02014b50);d.writeUInt16LE(20,4);d.writeUInt16LE(20,6);d.writeUInt16LE(0x800,8);d.writeUInt32LE(crc,16);d.writeUInt32LE(bytes.length,20);d.writeUInt32LE(bytes.length,24);d.writeUInt16LE(n.length,28);d.writeUInt32LE(offset,42);central.push(d,n);offset+=h.length+n.length+bytes.length;}
  const directory=Buffer.concat(central),end=Buffer.alloc(22);end.writeUInt32LE(0x06054b50);end.writeUInt16LE(entries.size,8);end.writeUInt16LE(entries.size,10);end.writeUInt32LE(directory.length,12);end.writeUInt32LE(offset,16);return Buffer.concat([...local,directory,end]);
}
