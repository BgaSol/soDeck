import {invoke} from './ui-observation';
import {useEffect,useRef,useState} from 'react';
import {core,context,dialogContext,readyDialog,completeDialog,closeDialog,errorMessage} from '@sodeck/sdk';
import {Button,EmptyState,Notice} from '@sodeck/ui';
import {Editor} from './editor';
import {draft,type Draft} from './editor-model';
import type {Rule} from './rules';
type Action='save'|'start'|'restart';
type Saved={ruleId:string;revision:number;action:Action};
type Data={original?:Rule;automatic:boolean;value:Draft;running:boolean};

export function EditorDialog(){
  const token=(dialogContext()?.initialState as {editorToken?:string})?.editorToken;
  const [data,setData]=useState<Data>(),[type,setType]=useState<Rule['type']>('network-proxy'),[values,setValues]=useState<Record<Rule['type'],Draft>>({'network-proxy':draft('network-proxy'),'port-forward':draft('port-forward')}),[busy,setBusy]=useState(false),[error,setError]=useState(''),[unknown,setUnknown]=useState(false);
  const pending=useRef<{submissionId:string;draft:Draft;action:Action}|undefined>(undefined),alive=useRef(true);
  useEffect(()=>{alive.current=true;void invoke<Data>('network.editor.get',{editorToken:token}).then(async data=>{if(!alive.current)return;setData(data);setType(data.value.type);setValues(old=>({...old,[data.value.type]:data.value}));await readyDialog();}).catch(e=>{if(alive.current){setError(errorMessage(e));void readyDialog();}});return()=>{alive.current=false;};},[token]);
  const close=()=>{void closeDialog().catch(e=>{if(alive.current)setError(errorMessage(e));});};
  const save=async(action:Action)=>{
    if(busy||!data)return;setBusy(true);setError('');
    const submitted=pending.current??{submissionId:crypto.randomUUID(),draft:{...values[type]},action};pending.current=submitted;
    try{
      let result:Saved;
      if(unknown){const status=await invoke<{status:string;result?:Saved}>('network.editor.result',{editorToken:token});if(status.status!=='committed'||!status.result)throw Object.assign(Error('保存结果仍待核实，请稍后重试'),{code:'RESULT_UNKNOWN'});result=status.result;}
      else result=await invoke<Saved>('network.editor.save',{editorToken:token,...submitted});
      await completeDialog(result);
    }catch(e){if(alive.current){setError(errorMessage(e));const uncertain=['RESULT_UNKNOWN','TIMEOUT'].includes((e as {code?:string}).code??'');setUnknown(uncertain);if(!uncertain)pending.current=undefined;}}
    finally{if(alive.current)setBusy(false);}
  };
  if(!data)return <EmptyState title={error?'无法打开编辑表单':'正在准备表单'}>{error&&<Notice kind="error">{error}</Notice>}<Button onClick={close}>关闭</Button></EmptyState>;
  const dirty=(['network-proxy','port-forward'] as const).some(kind=>JSON.stringify(values[kind])!==JSON.stringify(kind===data.value.type?data.value:draft(kind)));
  return <Editor value={values[type]} old={data.original} automatic={data.automatic} dirty={dirty} running={data.running} busy={busy} pending={unknown} error={error} onClose={close} onSave={action=>void save(action)} onChange={value=>{if(unknown)return;if(value.type!==type)setType(value.type);else setValues(old=>({...old,[type]:value}));}}/>;
}
