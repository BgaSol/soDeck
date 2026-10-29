export function byteText(bytes: Uint8Array) {let result='';for(let i=0;i<bytes.length;i+=8192)result+=String.fromCharCode(...bytes.subarray(i,i+8192));return result;}
export function rawBytes(value: string) {return Uint8Array.from(value, c => c.charCodeAt(0));}
export function displayText(value: string) {
  try {return decodeURIComponent([...value].map(c => '%' + c.charCodeAt(0).toString(16).padStart(2,'0')).join(''));}
  catch {return value.replace(/[^\x20-\x7e]/g, c => '\\x' + c.charCodeAt(0).toString(16).padStart(2,'0'));}
}
export interface Sections {values: Record<string,string>; errors: Record<string,string>; partial: boolean}
export function parseSections(bytes: Uint8Array, token: string, limits: Record<string,number>): Sections {
  const result: Sections = {values:{},errors:{},partial:false};
  let current: string | undefined, body: string[] = [], length = 0;
  const prefix = `SODECK_MON_V1 ${token} `;
  const seen = new Set<string>();
  // Decode one bounded line at a time: hostile runs of tiny lines must not
  // allocate a split array proportional to the whole output byte count.
  for (let offset=0; offset<bytes.length;) {
    const next=bytes.indexOf(10,offset),end=next<0?bytes.length:next,start=offset;offset=end+1;
    if(end-start>32768){if(current)result.errors[current]='采集行超过上限';result.partial=true;continue;}
    const line=byteText(bytes.subarray(start,end));
    if (line.startsWith(prefix)) {
      const [name, state, code] = line.slice(prefix.length).trim().split(' ');
      if (!(name in limits)) {result.partial=true;continue;}
      if (state === 'BEGIN') {
        if (current) {result.errors[current]='采集分段不完整';result.partial=true;}
        current=name;body=[];length=0;
        if(seen.has(name)){result.errors[name]='重复采集分段';delete result.values[name];result.partial=true;}
        seen.add(name);
      } else if (state === 'END' && current === name) {
        if(code!=='0')result.errors[name]='远端未提供此项采集能力';
        else if(length>limits[name])result.errors[name]='采集数据超过上限';
        else if(!result.errors[name])result.values[name]=body.join('\n').replace(/\n$/,'');
        current=undefined;body=[];
      } else {if(current)result.errors[current]='采集边界无效';result.partial=true;}
    } else if(current) {
      length+=line.length+1;
      if(body.length>=4096){result.errors[current]='采集行数超过上限';result.partial=true;}
      if(length<=limits[current]+32768&&!result.errors[current])body.push(line);
    }
  }
  if(current)result.errors[current]='采集分段不完整';
  for(const name of Object.keys(limits))if(!(name in result.values)){result.errors[name]??='未取得有效样本';result.partial=true;}
  return result;
}
