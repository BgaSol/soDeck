import {useCallback,useEffect,useRef,useState} from 'react';
import {core,errorMessage,subscribeTheme,type ThemeCatalog} from '@sodeck/sdk';
import {ThemeChoice,Notice,Button,type BuiltinThemeId} from '@sodeck/ui';
import {Dialog} from './dialog';

export function AppearanceDialog({onClose}:{onClose():void}){
  const [preference,setPreference]=useState<ThemeCatalog>(),[busy,setBusy]=useState(false),[error,setError]=useState('');
  const alive=useRef(true),inflight=useRef(false),selectionFocus=useRef<HTMLElement|null>(null);
  useEffect(()=>{if(!busy&&selectionFocus.current?.isConnected&&document.activeElement===document.body)selectionFocus.current.focus();},[busy]);
  const refresh=useCallback(async()=>{try{const p=await core.call<ThemeCatalog>('ui.theme.catalog.get');if(alive.current)setPreference(old=>!old||BigInt(p.revision)>=BigInt(old.revision)?p:old);}catch(e){if(alive.current)setError(errorMessage(e));}},[]);
  useEffect(()=>{alive.current=true;void refresh();const off=subscribeTheme(()=>void refresh());return()=>{alive.current=false;off();};},[refresh]);
  const select=async(themeId:string)=>{
    if(!preference||inflight.current||themeId===preference.selectedThemeId&&!preference.testOverride)return;
    selectionFocus.current=document.activeElement as HTMLElement;inflight.current=true;setBusy(true);setError('');
    try{const p=await core.call<ThemeCatalog>('ui.theme.choose',{themeId,expectedRevision:preference.revision});if(alive.current)setPreference(old=>!old||BigInt(p.revision)>=BigInt(old.revision)?p:old);}
    catch(e){if(alive.current)setError(errorMessage(e));await refresh();}
    finally{inflight.current=false;if(alive.current)setBusy(false);}
  };
  return <Dialog title="外观" subtitle="选择你的工作台风格" className="appearance-dialog" dismissOnOutside onClose={onClose}>
    <div className="appearance-body"><fieldset className="appearance-options" disabled={!preference}><legend className="visually-hidden">界面主题</legend>
      {preference?.themes.map(theme=><ThemeChoice key={theme.id} id={theme.id as BuiltinThemeId} name="appearance-theme" selected={preference.selectedThemeId===theme.id} busy={busy} defaultTheme={preference.defaultThemeId===theme.id} onChange={()=>void select(theme.id)}/>)}
    </fieldset><p className="appearance-help" role="status">{busy?'正在应用':!preference?'正在读取外观设置':'立即应用到所有窗口，并记住你的选择。'}</p>
    {preference?.highContrast&&<Notice>当前使用系统高对比颜色，退出高对比后恢复所选外观。</Notice>}
    {preference?.testOverride&&<Notice>当前使用测试主题。选择任一内置主题即可恢复。</Notice>}
    {!!preference?.legacyPlugins.length&&<Notice>部分旧插件将在下次打开时应用外观。</Notice>}
    {!!preference?.partialStylePlugins.length&&<Notice>部分旧插件可实时换色，完整控件样式需更新插件后生效。</Notice>}
    {error&&<Notice kind="error" action={<Button onClick={()=>{setError('');void refresh();}}>重试读取</Button>}>{error}</Notice>}
    </div>
  </Dialog>;
}
