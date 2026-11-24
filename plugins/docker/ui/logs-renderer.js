import {LogLayout} from './logs-layout.js';
export function createLogRenderer(viewport,root,onDetail,onContext) {
  const layout=new LogLayout();let revision='',ids=[],following=true,anchor;let painting=false,disposed=false,generation=0;
  const localTime=value=>new Date(value).toLocaleString('zh-CN',{hour12:false,month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit'});
  function visibleAnchor(){const {top,bottom}=viewport.getBoundingClientRect();const node=[...root.querySelectorAll('.log-line')].find(n=>{const row=n.getBoundingClientRect();return row.bottom>top&&row.top<bottom;});return node?{id:node.dataset.id,pixel:top-node.getBoundingClientRect().top}:layout.anchor(viewport.scrollTop-root.offsetTop);}
  function line(row,showSource,contextual){const node=document.createElement('div');node.className='log-line'+(row.context?' is-context':'');node.dataset.id=row.id;
    const time=document.createElement('button');time.className='log-time';time.textContent=localTime(row.time??row.receivedAt);time.title=row.time?'源时间：'+row.time:'接收时间';time.onclick=()=>onDetail(row.id);node.append(time);
    const body=document.createElement('div');body.className='log-body';if(showSource){const source=document.createElement('span');source.className='log-origin';source.textContent=row.source;body.append(source);}
    const text=document.createElement('span');text.className='log-text'+(['ERROR','FATAL','WARN'].includes(row.level)?' level-'+row.level:'');let at=0;
    for(const [a,b] of row.matches||[]){text.append(document.createTextNode(row.text.slice(at,a)));const mark=document.createElement('mark');mark.textContent=row.text.slice(a,b);text.append(mark);at=b;}text.append(document.createTextNode(row.text.slice(at)));body.append(text);
    if(row.length>row.text.length||row.incomplete){const full=document.createElement('button');full.className='log-context';full.textContent=row.incomplete?'查看已保留部分':'查看全文';full.onclick=()=>onDetail(row.id);body.append(full);}
    if(onContext&&contextual&&row.matches?.length){const button=document.createElement('button');button.className='log-context';button.textContent='上下文';button.onclick=()=>onContext(row.id);body.append(button);}
    node.append(body);return node;}
  async function paint(client,state,showSource){if(disposed||painting||!state.revision)return;painting=true;const current=generation,scroll=viewport.scrollTop;
    try{const selection=getSelection();if(selection?.toString()&&root.contains(selection.anchorNode))return;
      const old=anchor||visibleAnchor();if(revision!==state.revision){const next=await client.layout(state.revision);if(disposed||current!==generation||scroll!==viewport.scrollTop)return;ids=next;layout.reset(ids);revision=state.revision;}
      following=state.following;const held=layout.restore(old);const top=following?Math.max(0,layout.offset(ids.length)-viewport.clientHeight):Math.max(0,held??viewport.scrollTop-root.offsetTop);
      const start=Math.max(0,layout.index(top)-8),end=Math.min(ids.length,layout.index(top+viewport.clientHeight)+20);const rows=await client.rows(revision,start,Math.max(30,end-start));if(disposed||current!==generation||scroll!==viewport.scrollTop)return;
      const before=document.createElement('div'),after=document.createElement('div');before.className=after.className='log-spacer';before.style.height=layout.offset(start)+'px';after.style.height=(layout.offset(ids.length)-layout.offset(start+rows.length))+'px';const nodes=rows.map(r=>line(r,showSource,state.mode==='search'||state.conditions?.only));root.replaceChildren(before,...nodes,after);nodes.forEach((n,i)=>layout.measure(start+i,n.getBoundingClientRect().height));before.style.height=layout.offset(start)+'px';after.style.height=(layout.offset(ids.length)-layout.offset(start+rows.length))+'px';
      if(following)viewport.scrollTop=viewport.scrollHeight;else if(old?.id){const position=layout.restore(old);if(position!==null){after.style.height=(parseFloat(after.style.height)+Math.max(0,position+root.offsetTop+viewport.clientHeight-viewport.scrollHeight))+'px';viewport.scrollTop=position+root.offsetTop;const node=nodes.find(n=>n.dataset.id===old.id);if(node)viewport.scrollTop+=node.getBoundingClientRect().top-viewport.getBoundingClientRect().top+old.pixel;}}anchor=undefined;
    }finally{painting=false;}
  }
  const resize=new ResizeObserver(()=>{if(disposed)return;anchor=anchor||visibleAnchor();layout.reset(ids,true);});resize.observe(viewport);
  return {paint,anchor:visibleAnchor,hold(){generation++;anchor=undefined;},jump(id,pixel=0){const index=ids.indexOf(id);if(index>=0){generation++;anchor={id,pixel};viewport.scrollTop=layout.offset(index)+root.offsetTop+pixel;}},reflow(){generation++;anchor=visibleAnchor();layout.reset(ids,true);},reset(){generation++;revision='';ids=[];layout.reset([]);root.replaceChildren();},dispose(){disposed=true;resize.disconnect();}};
}
