import {Button,Checkbox,Field,Input,Select,ThemeChoice,type BuiltinThemeId} from '@sodeck/ui';
import {settingFields,type SettingField} from '@sodeck/sdk/settings';
import {type PageDraft,type DraftValue,matchesSearch} from './model';
function Highlight({text,query}:{text:string;query:string}){
  const terms=query.normalize('NFKC').toLocaleLowerCase().trim().split(/\s+/u).filter(Boolean);
  if(!terms.length)return text;
  const chars=[...text],normalized=chars.map(c=>c.normalize('NFKC').toLocaleLowerCase()),folded=normalized.join(''),marked=new Set<number>();
  for(const term of terms)for(let start=folded.indexOf(term);start>=0;start=folded.indexOf(term,start+1)){let offset=0;normalized.forEach((c,i)=>{if(offset<start+term.length&&offset+c.length>start)marked.add(i);offset+=c.length;});}
  return chars.map((c,i)=>marked.has(i)?<mark key={i}>{c}</mark>:c);
}
export function SettingsForm({record,query,disabled,onChange}:{record:PageDraft;query:string;disabled:boolean;onChange(key:string,value:DraftValue):void}){
  const {definition}=record.page;
  const allowed=(field:SettingField,kind:'visibleWhen'|'enabledWhen')=>{const c=field[kind];if(!c)return true;const raw=record.draft[c.key];return (typeof c.equals==='number'&&typeof raw==='string'&&/^-?\d+$/.test(raw)?Number(raw):raw)===c.equals;};
  const reset=()=>settingFields(definition).forEach(field=>onChange(field.key,field.default));
  return <div className="settings-form"><div className="settings-heading"><h1>{definition.title}</h1><Button size="compact" variant="quiet" disabled={disabled} onClick={reset}>恢复本页默认值</Button></div>
    {definition.sections.map(section=><section key={section.id} className="settings-section"><h2>{section.title}</h2>{section.fields.filter(f=>allowed(f,'visibleWhen')).map(field=>{
      const id=`setting-${field.key}`,value=record.draft[field.key],inactive=disabled||!allowed(field,'enabledWhen');
      const match=query.trim()&&matchesSearch([field.label,field.description,...field.keywords??[]].join(' '),query);
      const help=[field.description,field.effect==='newSession'?'新建或重新连接后生效':field.effect==='restart'?'重启后生效':'应用后生效'].filter(Boolean).join(' · ');
      return <div key={field.key} className="settings-field" data-search-match={!!match}>
        {record.page.snapshot.pageId==='core:appearance'&&field.key==='themeId'?<fieldset className="appearance-options" disabled={inactive}><legend className="visually-hidden">{field.label}</legend>{field.options?.map(o=><ThemeChoice key={o.value} id={o.value as BuiltinThemeId} name="settings-theme" selected={value===o.value} defaultTheme={field.default===o.value} onChange={()=>onChange(field.key,o.value)}/>)}</fieldset>:
        <Field id={id} label={<Highlight text={field.label} query={query}/>} help={<Highlight text={help} query={query}/>} error={record.errors[field.key]}>
          {field.type==='boolean'?<Checkbox id={id} label="启用" checked={value===true} disabled={inactive} aria-describedby={`${id}-help`} onChange={e=>onChange(field.key,e.target.checked)}/>:
          field.type==='enum'?<Select id={id} value={String(value)} disabled={inactive} aria-invalid={!!record.errors[field.key]} aria-describedby={`${id}-help`} onChange={e=>onChange(field.key,e.target.value)}>{field.options?.map(o=><option key={o.value} value={o.value}>{o.label}</option>)}</Select>:
          <Input id={id} type="text" inputMode={field.type==='integer'?'numeric':undefined} value={String(value)} disabled={inactive} aria-invalid={!!record.errors[field.key]} aria-describedby={`${id}-help`} maxLength={field.type==='text'?field.maxLength:24} onChange={e=>onChange(field.key,e.target.value)}/>}
        </Field>}
      </div>;
    })}</section>)}
  </div>;
}
