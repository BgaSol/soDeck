import fs from 'node:fs';

export function currentPluginApi() {
  const source=fs.readFileSync(new URL('../crates/plugin-protocol/src/lib.rs',import.meta.url),'utf8');
  const version=source.match(/pub const API_VERSION: &str = "([^"]+)";/)?.[1];
  if(!version)throw new Error('Plugin SDK API version is missing');
  return version;
}
export function compatiblePluginApi(core, required) {
  const parse=value=>typeof value==='string'&&/^(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value)?value.split('.').map(BigInt):null;
  const a=parse(core),b=parse(required);
  return Boolean(a&&b&&a.every(n=>n<=4294967295n)&&b.every(n=>n<=4294967295n)&&a[0]===b[0]&&a[1]>=b[1]);
}
