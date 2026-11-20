import {outputText} from './task-presentation.js';
function setText(el,value){value=String(value ?? '');if(el.textContent!==value)el.textContent=value;}
export function createOutputReader({request,valid,accept,onBusy}) {
  const $=id=>document.getElementById(id);
  let taskId, sequence=0, mode='history', cursor='0', pages=[], page, seen='0', latestBytes='0', loading=false, retry;
  function paint() {
    $('previous').textContent=mode==='latest'?'从头查看':'上一页';
    $('previous').disabled=loading || (mode==='history' && !pages.length);
    $('next').disabled=loading || mode==='latest' || !page?.hasMore;
    $('latest').disabled=loading;
    setText($('page-number'),mode==='latest'?'最近一页':'第 '+(pages.length+1)+' 页');
    setText($('output-title'),mode==='latest'?'最近输出':'输出');
    $('output-new').hidden=!page || BigInt(latestBytes)<=BigInt(seen);
  }
  async function read(nextMode,nextCursor,nextPages) {
    if(!taskId || valid()===null)return;
    const id=taskId, ticket=++sequence, context=valid();
    loading=true;retry=()=>read(nextMode,nextCursor,nextPages);onBusy(true);paint();
    $('output-error').hidden=true;$('output-retry').hidden=true;
    try {
      const result=await request({action:'task',taskId:id,...(nextMode==='latest'?{tail:true}:{cursor:nextCursor})});
      if(ticket!==sequence || taskId!==id || context!==valid())return;
      if(!result.output)throw new Error('未返回输出页，请重试。');
      mode=nextMode;cursor=nextCursor;pages=nextPages;page=result.output;seen=page.savedBytes || result.outputInfo?.savedBytes || '0';
      latestBytes=BigInt(latestBytes)>BigInt(seen)?latestBytes:seen;
      const text=outputText(page) || '暂时没有输出。';
      setText($('detail-output'),text);$('detail-output').scrollTop=mode==='latest'?$('detail-output').scrollHeight:0;
      accept(result);
    } catch(error) {
      if(ticket===sequence && taskId===id && context===valid()) {setText($('output-error'),'输出读取失败：'+(error.message || error));$('output-error').hidden=false;$('output-retry').hidden=false;}
    } finally {
      onBusy(false);
      if(ticket===sequence) {loading=false;paint();}
    }
  }
  $('next').onclick=()=>{if(!loading && page?.hasMore)void read('history',page.nextCursor,[...pages,cursor]);};
  $('previous').onclick=()=>{if(!loading)void (mode==='latest'?read('history','0',[]):read('history',pages.at(-1),pages.slice(0,-1)));};
  $('latest').onclick=()=>{if(!loading)void read('latest','0',[]);};
  $('output-retry').onclick=()=>{if(!loading)void retry?.();};
  return {
    open(task) {
      this.close();taskId=task.kind==='exec'?task.taskId:null;
      setText($('detail-output'),'等待读取输出');latestBytes=task.outputInfo?.savedBytes || '0';paint();
      if(task.kind==='exec')void read('history','0',[]);
    },
    update(task) {latestBytes=task.outputInfo?.savedBytes || latestBytes;paint();},
    suspend() {sequence++;loading=false;paint();},
    resume() {if(taskId && !page)void read('history','0',[]);},
    close() {sequence++;taskId=null;mode='history';cursor='0';pages=[];page=null;seen=latestBytes='0';loading=false;retry=null;$('output-error').hidden=true;$('output-retry').hidden=true;$('output-new').hidden=true;}
  };
}

