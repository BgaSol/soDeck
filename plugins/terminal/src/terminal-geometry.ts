import type {Terminal} from '@xterm/xterm';
import type {FitAddon} from '@xterm/addon-fit';
type Binding={ptyId:string;generation:number};
type Size=Binding&{cols:number;rows:number};
const identity=(b:Binding)=>`${b.ptyId}:${b.generation}`;
const key=(s:Size)=>`${identity(s)}:${s.cols}:${s.rows}`;

/** One measured grid, one animation-frame fit, one in-flight PTY resize. */
export function terminalGeometry({element,term,fit,enabled,binding,resize,error}:{element:HTMLElement;term:Terminal;fit:FitAddon;enabled():boolean;binding():Binding|null;resize(size:Size):Promise<unknown>;error(error:unknown):void}){
  let disposed=false,frame=0,flight=false,last='',queued:Size|undefined;
  const flush=async()=>{
    if(flight||disposed||!enabled())return;
    flight=true;
    try{while(queued&&!disposed&&enabled()){
      const size=queued;queued=undefined;const current=binding();
      if(!current||identity(current)!==identity(size)||last===key(size))continue;
      try{await resize(size);if(!disposed&&binding()&&identity(binding()!)===identity(size))last=key(size);}
      catch(e){if(!disposed&&binding()&&identity(binding()!)===identity(size)){last='';error(e);}}
    }}finally{flight=false;}
  };
  const measure=()=>{
    frame=0;if(disposed||!enabled()||document.visibilityState==='hidden'||!element.isConnected||element.clientWidth<30||element.clientHeight<30)return;
    const dimensions=fit.proposeDimensions();if(!dimensions||dimensions.cols<2||dimensions.rows<1)return;
    const buffer=term.buffer.active,atBottom=buffer.viewportY>=buffer.baseY,previous=buffer.viewportY;
    const marker=atBottom?undefined:term.registerMarker(previous-buffer.baseY-buffer.cursorY);
    if(dimensions.cols!==term.cols||dimensions.rows!==term.rows){
      fit.fit();
      if(atBottom)term.scrollToBottom();else term.scrollToLine(marker&&!marker.isDisposed?marker.line:previous);
    }
    marker?.dispose();
    const active=binding();if(active?.ptyId){queued={...active,cols:term.cols,rows:term.rows};void flush();}
  };
  const schedule=()=>{if(!disposed&&!frame)frame=requestAnimationFrame(measure);};
  const observer=new ResizeObserver(schedule);observer.observe(element);
  document.fonts?.ready.then(schedule).catch(()=>{});document.fonts?.addEventListener('loadingdone',schedule);
  document.addEventListener('visibilitychange',schedule);window.addEventListener('resize',schedule);window.addEventListener('focus',schedule);
  let resolution=matchMedia(`(resolution: ${devicePixelRatio}dppx)`);
  const dpi=()=>{resolution.removeEventListener('change',dpi);resolution=matchMedia(`(resolution: ${devicePixelRatio}dppx)`);resolution.addEventListener('change',dpi);schedule();};
  resolution.addEventListener('change',dpi);schedule();
  return {schedule,dispose(){disposed=true;queued=undefined;if(frame)cancelAnimationFrame(frame);observer.disconnect();resolution.removeEventListener('change',dpi);document.fonts?.removeEventListener('loadingdone',schedule);document.removeEventListener('visibilitychange',schedule);window.removeEventListener('resize',schedule);window.removeEventListener('focus',schedule);}};
}
