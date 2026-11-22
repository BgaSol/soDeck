export async function readDocument(data, path) {
  for(let attempt=0;attempt<3;attempt++){
    let info=await data('draft.describe');if(!info.path)info=await data('document.load',{path});
    if(!Number.isSafeInteger(info.size)||info.size<0||info.size>1048576)throw new Error('文件正文大小无效');
    const bytes=new Uint8Array(info.size);let changed=false;
    for(let offset=0;offset<bytes.length;offset+=16384){
      let block;
      try {block=await data('draft.chunk',{offset:String(offset),draftRef:info.draftRef});}
      catch(error){if(error.data?.code!=='revisionConflict')throw error;changed=true;break;}
      if(JSON.stringify(block.draftRef)!==JSON.stringify(info.draftRef)){changed=true;break;}
      if(!Array.isArray(block.data)||block.data.length!==Math.min(16384,bytes.length-offset))throw new Error('文件正文数据块不完整');
      bytes.set(block.data,offset);
    }
    if(!changed)return {info,bytes};
  }
  throw new Error('文件草稿在读取期间持续变化，请重新打开编辑器');
}
