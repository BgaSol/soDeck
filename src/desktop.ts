import {invoke} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';
import type {Window} from '@tauri-apps/api/window';
import type {Shell} from './shell';

interface DesktopState {
  generation:number;
  visible:boolean;
  blocked:boolean;
  trayAvailable?:boolean;
  navigation:{id:number;target:'messages'|'settings'}|null;
  background:{id:number;generation:number}|null;
}

/** Native state is recoverable; events only wake this reader. */
export class DesktopUi {
  private listeners:UnlistenFn[]=[];
  private disposed=false;
  private reading?:Promise<void>;
  private again=false;
  private state?:DesktopState;
  private background=0;
  private navigation=0;
  constructor(private win:Window, private shell:Shell, private navigate:(target:'messages'|'settings')=>void, private prepare:()=>Promise<boolean>,private notify:(message:string)=>void){}
  async listen() {
    this.listeners.push(await listen('desktop-state-changed',()=>void this.refresh().catch(e=>this.notify(String(e)))));
    document.addEventListener('visibilitychange',this.visibility);
  }
  async ready() {
    const state=await invoke<DesktopState>('desktop_state');
    this.apply(await invoke<DesktopState>('desktop_ready',{generation:state.generation}));
    if(state.trayAvailable===false)this.notify(this.shell.snapshot?.os==='linux'?'系统托盘不可用，可通过任务栏或窗口切换返回。':'系统托盘不可用，关闭主窗口将退出应用。');
    await this.refresh();
  }
  private visibility=()=>{if(this.state)this.shell.setWindowVisible(this.state.visible&&document.visibilityState!=='hidden');};
  private apply(state:DesktopState){this.state=state;this.visibility();}
  refresh():Promise<void> {
    if(this.reading){this.again=true;return this.reading;}
    this.reading=(async()=>{do{
      this.again=false;
      const state=await invoke<DesktopState>('desktop_state');
      if(this.disposed)return;
      this.apply(state);
      if(this.win.label!=='main')continue;
      const pending=state.background;
      if(pending&&pending.id!==this.background){
        this.background=pending.id;
        let allow=false;
        try {allow=await this.prepare();if(allow)this.shell.setWindowVisible(false);else this.notify('请先处理当前对话框或页面交接，再收起窗口。');
          await invoke('desktop_background_done',{generation:pending.generation,id:pending.id,allow});
        }catch(error){this.notify(String(error));}finally{this.apply(await invoke<DesktopState>('desktop_state'));this.again=true;}
      }
      if(!state.blocked&&state.navigation&&state.navigation.id!==this.navigation){
        this.navigate(state.navigation.target);
        await invoke('desktop_navigation_done',{generation:state.generation,id:state.navigation.id});
        this.navigation=state.navigation.id;
      }
    }while(this.again&&!this.disposed);})().finally(()=>{this.reading=undefined;});
    return this.reading;
  }
  dispose(){this.disposed=true;this.listeners.forEach(fn=>fn());document.removeEventListener('visibilitychange',this.visibility);}
}

export const chooseLocalPath=(kind:'privateKey'|'pluginDirectory')=>invoke<string|null>('choose_local_path',{kind});
