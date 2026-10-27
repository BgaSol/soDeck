import {posix} from 'node:path';

export function boundaryErrors(file,text){
  file=file.replaceAll('\\','/');const errors=[];
  const owner=/^plugins\/([^/]+)\//.exec(file)?.[1];
  if(owner){
    for(const match of text.matchAll(/(?:\bfrom\s*|\bimport\s*(?:\(\s*)?|\brequire\s*\(\s*)["']([^"']+)["']/g)){
      const specifier=match[1].replaceAll('\\','/');
      const resolved=specifier.startsWith('.')?posix.normalize(posix.join(posix.dirname(file),specifier)):specifier;
      const peer=/^plugins\/([^/]+)\//.exec(resolved)?.[1];
      if(peer&&peer!==owner||/^(?:apps\/desktop|crates\/|@tauri-apps\/)/.test(resolved)||/^@sodeck\/(?!sdk(?:\/|$)|ui(?:\/|$))/.test(specifier))errors.push(`forbidden plugin dependency: ${specifier}`);
    }
  }
  if(file.startsWith('crates/core/')){
    if(/(?:use|mod)\s+(?:docker|netbird|terminal_plugin|files_plugin|mounts|linux_collector)\b/.test(text)||/(?:include_str!|include!)\s*\(\s*"[^"\n]*plugins\//.test(text))errors.push('core imports plugin business');
  }
  if(file.startsWith('crates/communication/')&&/\b(?:sodeck_core|sodeck_plugin|tauri|shell_integration|directory_reporter)\b/.test(text))errors.push('communication imports host or plugin business');
  if(file.startsWith('packages/sdk/')&&/(?:from|import)\s*["'][^"']*(?:plugins\/|apps\/desktop|crates\/)/.test(text))errors.push('SDK imports private application implementation');
  if(file.startsWith('packages/ui/')&&/(?:from|import)\s*["'][^"']*(?:plugins\/|apps\/desktop|crates\/|@sodeck\/sdk)/.test(text))errors.push('UI imports business or runtime services');
  if(file.startsWith('packages/sdk/')&&/(?:from|import)\s*["'](?:react(?:-dom)?(?:\/|["'])|@sodeck\/ui)/.test(text))errors.push('SDK imports React or UI');
  return errors;
}
