// Only the source Surface can resolve paths through its own file resource.
let epoch=0;
if(typeof window!=='undefined')for(const name of ['contextChanged','unmount','flush'])window.addEventListener('sodeck:'+name,()=>epoch++);
export async function openRemoteFile({path,mount,fileId,viewType='editor',request=(method,params)=>sodeck.request(method,params)}) {
 const ticket=++epoch,context=structuredClone(mount.context);
 const check=()=>{if(ticket!==epoch)throw new Error('文件打开上下文已变化，请重新打开');};
 if(!fileId){const result=await request('event',{action:'fileEditor.resource',expectedContext:context});fileId=result.data.id;check();}
 const info=await request('data',{op:'files',id:fileId,action:'stat',path});check();
 if(info.attrs.kind!=='file')throw new Error('此条目不是普通文件');
 if(Number(info.attrs.size)>1048576)throw new Error('文件超过 1 MiB，无法编辑，请下载后查看');
 const intent={action:'fileEditor.open',requestId:crypto.randomUUID(),expectedContext:context,path:info.path,viewType};
 // Reuse the same open identity if the first observation was lost.
 try{return (await request('event',intent)).data;}catch(cause){check();try{return (await request('event',intent)).data;}catch{throw cause;}}
}
