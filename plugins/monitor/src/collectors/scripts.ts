import {utf8} from '@sodeck/sdk/bytes';
import {rawBytes} from './framing';
import type {Mount} from '../protocol';
export const FAST_LIMITS = {before:128,cpu:4096,memory:32768,load:256,net:65536,disk:65536,after:128};
export const INVENTORY_LIMITS = {os:128,host:512,kernel:512,boot:128,online:4096,mounts:393216,interfaces:16384,devices:32768};
function prelude(token: string) {
  if(!/^[a-zA-Z0-9-]{1,96}$/.test(token))throw Error('Invalid sampler token');
  return `LC_ALL=C; PATH=/usr/sbin:/usr/bin:/sbin:/bin; export LC_ALL PATH\nTOKEN='${token}'\nbegin(){ printf 'SODECK_MON_V1 %s %s BEGIN\\n' "$TOKEN" "$1"; }\nend(){ printf '\\nSODECK_MON_V1 %s %s END %s\\n' "$TOKEN" "$1" "$2"; }\npart(){ begin "$1"; head -c "$3" "$2" 2>/dev/null; rc=$?; end "$1" "$rc"; }\n`;
}
export function fastScript(token: string, skip: readonly string[] = []) {
  let body = `part before /proc/uptime 128\nbegin cpu; head -n 1 /proc/stat 2>/dev/null; rc=$?; end cpu "$rc"\npart memory /proc/meminfo 32769\npart load /proc/loadavg 257\npart net /proc/net/dev 65537\npart disk /proc/diskstats 65537\npart after /proc/uptime 128\n`;
  for(const name of skip){if(name==='cpu')body=body.replace(/begin cpu;[^\n]*\n/,'');else body=body.replace(new RegExp('part '+name+' [^\n]*\n'),'');}
  return utf8(prelude(token)+body);
}
export function inventoryScript(token: string) {
  return utf8(prelude(token)+`begin os; uname -s; rc=$?; end os "$rc"\npart host /proc/sys/kernel/hostname 512\npart kernel /proc/sys/kernel/osrelease 512\npart boot /proc/sys/kernel/random/boot_id 128\npart online /sys/devices/system/cpu/online 4097\npart mounts /proc/self/mountinfo 393217\nbegin interfaces\ni=0\nfor p in /sys/class/net/*; do\n [ -r "$p/ifindex" ] || continue\n i=$((i+1)); [ "$i" -le 65 ] || break\n IFS= read -r idx < "$p/ifindex"\n physical=0; [ -e "$p/device" ] && physical=1\n printf '%s %s %s\\n' "\${p##*/}" "$idx" "$physical"\ndone\nend interfaces 0\nbegin devices\ni=0\nfor p in /sys/class/block/*; do\n [ -r "$p/dev" ] || continue\n i=$((i+1)); [ "$i" -le 65 ] || break\n IFS= read -r dev < "$p/dev"\n partition=0; [ -r "$p/partition" ] && partition=1\n printf '%s %s %s\\n' "\${p##*/}" "$dev" "$partition"\ndone\nend devices 0\n`);
}
function quoteRaw(path: string) {
  if(!path.startsWith('/')||path.includes('\0')||path.length>16384)throw Error('无效挂载路径');
  return "'"+path.replace(/'/g,"'\\''")+"'";
}
export function capacityScript(token: string, mount: Mount) {
  if(!/^\d+$/.test(mount.id))throw Error('无效挂载身份');
  const identity = `while IFS= read -r row; do case "$row" in '${mount.id} '*) printf '%s\\n' "$row"; break;; esac; done < /proc/self/mountinfo`;
  const ascii = prelude(token)+`begin before; ${identity}; end before 0\nbegin capacity\npath=${quoteRaw(mount.rawPath)}\nvalid_stat=0\nif values=$(stat -f -c '%S %b %f %a' "$path" 2>/dev/null); then\n set -f; set -- $values\n if [ "$#" -eq 4 ]; then\n  valid_stat=1; for value in "$@"; do case "$value" in ''|*[!0-9]*) valid_stat=0;; esac; done\n fi\nfi\nif [ "$valid_stat" -eq 1 ]; then\n printf 'STAT %s\\n' "$values"; rc=0\nelse\n printf 'DF\\n'; df -Pk "$path" 2>/dev/null; rc=$?\nfi\nend capacity "$rc"\nbegin after; ${identity}; end after 0\n`;
  const bytes=rawBytes(ascii);if(bytes.length>65536)throw Error('采集脚本超过上限');return bytes;
}
