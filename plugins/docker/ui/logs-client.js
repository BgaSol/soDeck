export function createLogClient(valid) {
  let next=1,reading=Promise.resolve(),disposed=false;
  const read=query=>{const result=reading.then(()=>{if(disposed)throw Error('日志页面已卸载');return sodeck.request('read',query);});reading=result.catch(()=>{});return result;};
  const command=async(action,params={})=>{if(!valid())throw Error('日志页面当前不可操作');return sodeck.request('event',{action,...params});};
  return {
    read,command,dispose(){disposed=true;},
    update(state){next=Math.max(next,Number(state.nextIntent)||1);},
    intent(action,params={}){return command(action,{requestId:crypto.randomUUID(),intentSequence:String(next++),...params});},
    async layout(revision){const ids=[];let total;do{const page=await read({op:'layout',revision,start:ids.length});if(page.revision!==revision||page.next<=ids.length&&page.total>ids.length)throw Error('日志布局页不连续');ids.push(...page.ids);total=page.total;}while(ids.length<total);return ids;},
    async rows(revision,start,count){const rows=[];let total=start+count;while(start+rows.length<total){const page=await read({op:'window',revision,start:start+rows.length,count:total-start-rows.length});if(page.revision!==revision)throw Error('日志投影已更新');rows.push(...page.rows);total=Math.min(total,page.total);if(!page.rows.length)break;}return rows;},
    async detail(record){let offset=0,blocks=[],info;do{info=await read({op:'detail',record,offset});const bytes=decode(info.dataBase64);if(info.nextOffset!==offset+bytes.length)throw Error('日志详情字节不连续');blocks.push(bytes);offset=info.nextOffset;}while(!info.done);const bytes=new Uint8Array(offset);let at=0;for(const block of blocks){bytes.set(block,at);at+=block.length;}return {...info,raw:new TextDecoder().decode(bytes)};}
  };
}
export function decode(value){return Uint8Array.from(atob(value),c=>c.charCodeAt(0));}
