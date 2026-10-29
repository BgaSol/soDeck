import {createPortal} from 'react-dom';
import {useEffect,useLayoutEffect,useId,useRef,useState,type InputHTMLAttributes,type ReactNode} from 'react';
import {Eye,EyeOff,ChevronDown,Check} from 'lucide-react';
import {Field,Input,InputGroup,IconButton,Button} from './controls';

export function PasswordField({label,help,error,id:provided,...props}:Omit<InputHTMLAttributes<HTMLInputElement>,'style'|'type'>&{label:string;help?:ReactNode;error?:string}){
  const generated=useId(),id=provided??generated;const [visible,setVisible]=useState(false);
  return <Field id={id} label={label} help={help} error={error}><InputGroup><Input {...props} id={id} type={visible?'text':'password'} autoComplete="new-password" spellCheck={false} aria-invalid={!!error} aria-describedby={[help?`${id}-help`:'',error?`${id}-error`:''].filter(Boolean).join(' ')||undefined}/><IconButton label={`${visible?'隐藏':'显示'}${label}`} icon={visible?EyeOff:Eye} variant="quiet" disabled={props.disabled} aria-pressed={visible} onClick={()=>setVisible(v=>!v)}/></InputGroup></Field>;
}
export type SearchOption={value:string;label:string;detail?:string};
export function SearchSelect({label,value,options,onChange,help,error,placeholder='搜索并选择',empty='没有匹配项',disabled=false,loading=false,id:provided}:{label:string;value:string;options:readonly SearchOption[];onChange(value:string):void;help?:ReactNode;error?:string;placeholder?:string;empty?:string;disabled?:boolean;loading?:boolean;id?:string}){
  const generated=useId(),id=provided??generated,root=useRef<HTMLDivElement>(null),input=useRef<HTMLInputElement>(null);
  const panel=useRef<HTMLDivElement>(null);
  const [position,setPosition]=useState({left:0,top:0,width:0,maxHeight:320});
  const [open,setOpen]=useState(false),[query,setQuery]=useState(''),[active,setActive]=useState(0);
  useLayoutEffect(()=>{
    if(!open||disabled)return;
    const place=()=>{const box=root.current?.getBoundingClientRect();if(!box)return;
      if(box.bottom<0||box.top>window.innerHeight){setOpen(false);return;}
      const below=window.innerHeight-box.bottom-8,above=box.top-8,up=below<180&&above>below;
      const height=Math.min(320,Math.max(80,up?above:below));
      setPosition({left:Math.max(8,Math.min(box.left,window.innerWidth-box.width-8)),top:up?Math.max(8,box.top-Math.min(height,panel.current?.scrollHeight??height)):box.bottom,width:Math.min(box.width,window.innerWidth-16),maxHeight:height});
    };
    const outside=(e:PointerEvent)=>{if(!root.current?.contains(e.target as Node)&&!panel.current?.contains(e.target as Node)){setOpen(false);setQuery('');}};
    place();window.addEventListener('resize',place);window.addEventListener('scroll',place,true);document.addEventListener('pointerdown',outside);
    return()=>{window.removeEventListener('resize',place);window.removeEventListener('scroll',place,true);document.removeEventListener('pointerdown',outside);};
  },[open,disabled]);
  const selected=options.find(o=>o.value===value),matches=options.filter(o=>(o.label+' '+(o.detail??'')).toLowerCase().includes(query.toLowerCase()));
  const choose=(option:SearchOption)=>{onChange(option.value);setOpen(false);setQuery('');input.current?.focus();};
  useEffect(()=>{if(open)document.getElementById(`${id}-option-${active}`)?.scrollIntoView({block:'nearest'});},[active,open,id]);
  const start=Math.max(0,Math.min(active-4,matches.length-9));
  return <Field id={id} label={label} help={help} error={error}><div ref={root} className="sd-search-select" onBlur={e=>{if(!e.currentTarget.contains(e.relatedTarget)&&!panel.current?.contains(e.relatedTarget)){setOpen(false);setQuery('');}}}>
    <InputGroup><Input ref={input} id={id} role="combobox" aria-expanded={open} aria-controls={`${id}-options`} aria-autocomplete="list" aria-activedescendant={open&&matches[active]?`${id}-option-${active}`:undefined} aria-invalid={!!error} aria-describedby={[help?`${id}-help`:'',error?`${id}-error`:''].filter(Boolean).join(' ')||undefined} value={open?query:selected?.label??''} placeholder={placeholder} disabled={disabled} autoComplete="off" onFocus={()=>{setOpen(true);setQuery('');setActive(0);}} onChange={e=>{setQuery(e.target.value);setActive(0);setOpen(true);}} onKeyDown={e=>{
      if(e.nativeEvent.isComposing)return;
      if(e.key==='Escape'){e.preventDefault();e.stopPropagation();setOpen(false);setQuery('');}
      if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();setOpen(true);setActive(a=>Math.max(0,Math.min(matches.length-1,a+(e.key==='ArrowDown'?1:-1))));}
      if(e.key==='Enter'&&open){e.preventDefault();e.stopPropagation();if(matches[active])choose(matches[active]);}
      if(e.key==='Home'&&open){e.preventDefault();setActive(0);}if(e.key==='End'&&open){e.preventDefault();setActive(matches.length-1);}
    }}/><IconButton label={`展开${label}`} icon={ChevronDown} variant="quiet" disabled={disabled} onMouseDown={e=>e.preventDefault()} onClick={()=>{const next=!open;input.current?.focus();setOpen(next);setQuery('');}}/></InputGroup>
    {open&&!disabled&&createPortal(<div ref={panel} style={position} className="sd-search-options" id={`${id}-options`} role="listbox" aria-label={label} onWheel={e=>{if(matches.length>9)setActive(a=>Math.max(0,Math.min(matches.length-1,a+(e.deltaY>0?3:-3))));}}>
      {loading?<div role="status" className="sd-search-empty">正在加载</div>:!matches.length?<div role="status" className="sd-search-empty">{empty}</div>:<>
        {start>0&&<Button variant="quiet" size="compact" onMouseDown={e=>e.preventDefault()} onClick={()=>setActive(a=>Math.max(0,a-8))}>前面的选项</Button>}
        {matches.slice(start,start+9).map((o,index)=><div id={`${id}-option-${start+index}`} key={o.value} role="option" aria-selected={o.value===value} aria-posinset={start+index+1} aria-setsize={matches.length} className="sd-search-option" data-active={active===start+index} onMouseDown={e=>e.preventDefault()} onClick={()=>choose(o)}><span>{o.value===value&&<Check aria-hidden="true" size={16}/>} {o.label}</span>{o.detail&&<small>{o.detail}</small>}</div>)}
        {start+9<matches.length&&<Button variant="quiet" size="compact" onMouseDown={e=>e.preventDefault()} onClick={()=>setActive(a=>Math.min(matches.length-1,a+8))}>后面的选项</Button>}
        <div className="sd-search-count">共 {matches.length} 项 · 输入可筛选</div>
      </>}
    </div>,root.current?.closest('.sd-theme-scope')??document.body)}
  </div></Field>;
}
