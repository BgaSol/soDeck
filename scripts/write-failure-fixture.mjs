// Block the final target with a directory while keeping the prior file for recovery assertions.
import fs from 'node:fs';
export function blockTarget(file) {
  if(fs.existsSync(file)){if(!fs.statSync(file).isFile())throw Error('Fixture target is not an ordinary file');fs.renameSync(file,file+'.fixture-backup');}
  fs.mkdirSync(file);
}
export function restoreTarget(file) {
  fs.rmdirSync(file);
  if(fs.existsSync(file+'.fixture-backup'))fs.renameSync(file+'.fixture-backup',file);
}
