export async function readDocument(data, path) {
  for(let attempt=0;attempt<3;attempt++){
    let info=await data('describe');if(!info.path)info=await data('read',{path});
    if(!Number.isSafeInteger(info.size)||info.size<0||info.size>1048576)throw new Error('文件正文大小无效');
    const bytes=new Uint8Array(info.size);let changed=false;
    for(let offset=0;offset<bytes.length;offset+=16384){
      const block=await data('chunk',{offset});
      if(block.revision!==info.revision||block.size!==info.size){changed=true;break;}
      if(!Array.isArray(block.data)||block.data.length!==Math.min(16384,bytes.length-offset))throw new Error('文件正文数据块不完整');
      bytes.set(block.data,offset);
    }
    if(!changed)return {info,bytes};
  }
  throw new Error('文件草稿在读取期间持续变化，请重新打开编辑器');
}
