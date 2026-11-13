// One upload and one explicit action snapshot; failed attempts never poison later work.
export function createDraftSync({data, read, info, editRevision=0, changed=()=>{}, error=()=>{}}) {
 let revision=editRevision, committedRevision=editRevision, reference=info.draftRef;
 let sequence=0n, attempt, flight, actionActive=false, disposed=false;
 const capture=()=>({revision,bytes:new Uint8Array(read())});
 const hash=async bytes=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),b=>b.toString(16).padStart(2,'0')).join('');
 async function upload(snapshot) {
  if(snapshot.revision===committedRevision)return reference;
  if(snapshot.bytes.length>1048576)throw new Error('草稿超过 1 MiB，无法同步；请缩小内容');
  if(!attempt||attempt.revision!==snapshot.revision)attempt={...snapshot,uploadId:crypto.randomUUID(),uploadSequence:String(++sequence),sha256:await hash(snapshot.bytes)};
  const a=attempt;
  const identity={documentId:reference.documentId,uploadId:a.uploadId,uploadSequence:a.uploadSequence};
  let result;
  try {
   result=await data('draft.begin',{...identity,editRevision:String(a.revision),size:a.bytes.length,sha256:a.sha256});
   if(result.state!=='committed') {
    for(let offset=result.nextOffset;offset<a.bytes.length;offset+=16384)await data('draft.append',{...identity,offset,data:Array.from(a.bytes.subarray(offset,offset+16384))});
    result=await data('draft.commit',identity);
   }
  } catch(cause) {
   try { result=await data('draft.status',{uploadId:a.uploadId}); } catch { throw cause; }
   if(result.state!=='committed')throw cause;
  }
  if(result.uploadId!==a.uploadId||result.sha256!==a.sha256||result.editRevision!==String(a.revision)||!result.draftRef)throw new Error('草稿回执身份不匹配');
  reference=result.draftRef;committedRevision=a.revision;attempt=undefined;
  changed({revision,committedRevision,reference});
  return reference;
 }
 async function syncSnapshot(snapshot) {
  if(flight)await flight.catch(()=>{});
  if(disposed)throw new Error('文件页面已卸载');
  const work=upload(snapshot);flight=work;
  try{return await work;}finally{if(flight===work)flight=undefined;}
 }
 function pump() {
  if(actionActive||flight||disposed)return;
  const snapshot=capture();
  void syncSnapshot(snapshot).then(()=>{if(revision!==snapshot.revision)pump();},error);
 }
 return {
  change(){revision++;changed({revision,committedRevision,reference});pump();},
  async flush(){return this.action(async ref=>ref);},
  async action(accept) {
   if(actionActive)throw new Error('另一项正文操作正在确认，请稍后重试');
   actionActive=true;const snapshot=capture();
   try{return await accept(await syncSnapshot(snapshot));}
   finally{actionActive=false;if(revision!==snapshot.revision)pump();}
  },
  state:()=>({revision,committedRevision,reference}),
  dispose(){disposed=true;},
 };
}
