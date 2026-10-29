import {readFile} from 'node:fs/promises';

/** Import named definitions with their references; collisions must be identical. */
export async function loadContract(url, stack=[]) {
  if(stack.includes(url.href))throw Error(`Contract import cycle: ${url.href}`);
  const schema=JSON.parse(await readFile(url,'utf8'));
  const merge=(name,value)=>{
    if(schema.$defs[name]&&JSON.stringify(schema.$defs[name])!==JSON.stringify(value))throw Error(`Conflicting contract definition ${name} in ${url.href}`);
    schema.$defs[name]=value;
  };
  if(schema['x-extends']) {
    const parent=await loadContract(new URL(schema['x-extends'],url),[...stack,url.href]);
    for(const [name,value] of Object.entries(parent.$defs))merge(name,value);
  }
  for(const entry of schema['x-imports']??[]) {
    const source=await loadContract(new URL(entry.source,url),[...stack,url.href]);
    const visited=new Set();
    const add=name=>{
      if(visited.has(name))return;
      visited.add(name);
      const value=source.$defs[name];if(!value)throw Error(`Missing contract definition ${name}`);
      merge(name,value);
      const walk=item=>{if(!item||typeof item!=='object')return;if(item.$ref)add(item.$ref.split('/').at(-1));for(const child of Object.values(item))walk(child);};
      walk(value);
    };
    entry.names.forEach(add);
  }
  return schema;
}
