import {invoke} from '@tauri-apps/api/core';
import {escape as e, button as b} from './ui';
import {core} from './core';

interface StartupState {enabled:boolean;state:'enabled'|'disabled'|'blocked'|'stale'|'unknown'|'unavailable';reason:string|null}

/** Only the application settings page reads OS registration; ordinary status polling does not. */
export class ApplicationSettings {
  private state?:StartupState;
  private active=false;
  private reading=false;
  private saving=false;
  private requested=false;
  private closeSaving=false;
  private error='';
  private feedback='';
  private disposed=false;
  private revision=0;
  constructor(private changed:()=>void,private refreshApplication:()=>Promise<unknown>){window.addEventListener('focus',this.focus);}
  private focus=()=>{if(this.active)void this.refresh();};
  mount(active:boolean){const entered=active&&!this.active;this.active=active;if(entered)void this.refresh();}
  refresh=async()=>{
    if(this.disposed||this.reading||this.saving)return;
    this.reading=true;const revision=++this.revision;this.changed();
    try{const state=await invoke<StartupState>('login_startup_state');if(revision===this.revision){this.state=state;this.error='';}}
    catch(error){if(revision===this.revision){this.error=String(error);this.state=undefined;}}
    finally{this.reading=false;if(!this.disposed)this.changed();}
  };
  render(os:string){
    const disabled=this.reading||this.saving||!this.state||['unknown','unavailable'].includes(this.state.state);
    const text=this.saving?'正在保存启动设置…':this.reading?'正在读取启动设置…':this.error||this.state?.reason||this.feedback||'';
    const description=os==='linux'?'登录系统后自动最小化运行，可从任务栏或窗口切换返回。':os==='macos'?'登录系统后自动在菜单栏运行，不打开主窗口。':'登录系统后自动在托盘运行，不打开主窗口。';
    return `<div class="setting-row" data-setting-type="boolean"><div><label for="login-startup">开机启动并驻留托盘</label><p id="login-startup-description">${description}</p></div><label class="setting-switch"><input id="login-startup" type="checkbox" role="switch" aria-describedby="login-startup-description login-startup-feedback" ${(this.saving?this.requested:this.state?.enabled)?'checked':''} ${disabled?'disabled':''}><span aria-hidden="true"></span></label></div><div id="login-startup-feedback" role="status" aria-live="polite" class="hint ${this.error?'error-text':''}">${e(text)} ${!this.reading&&!this.saving&&(this.error||this.state?.state==='unknown')?b('login-startup-retry','重新读取',undefined,'quiet'):''}</div>`;
  }
  input(input:HTMLInputElement):boolean{
    if(input.id==='close-behavior'){
      const value=input.value;if(value!=='background'&&value!=='exit')return true;
      if(this.closeSaving)return true;
      this.closeSaving=true;input.disabled=true;
      void core.closeBehavior(value).catch(error=>{this.error=String(error);}).finally(async()=>{this.closeSaving=false;await this.refreshApplication();this.changed();});
      return true;
    }
    if(input.id!=='login-startup')return false;
    if(this.saving||this.reading)return true;
    const enabled=input.checked;this.requested=enabled;this.saving=true;this.error='';this.feedback='';++this.revision;this.changed();
    void (async()=>{
      try{this.state=await invoke<StartupState>('set_login_startup',{enabled});this.feedback=this.state.state==='enabled'?'已开启，下次登录系统时生效。':this.state.state==='disabled'?'已关闭，不影响当前运行。':'';}
      catch(error){this.error=String(error);try{this.state=await invoke<StartupState>('login_startup_state');}catch{this.state=undefined;}}
      finally{this.saving=false;if(!this.disposed)this.changed();}
    })();
    return true;
  }
  dispose(){this.disposed=true;++this.revision;window.removeEventListener('focus',this.focus);}
}
