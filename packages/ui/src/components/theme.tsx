import {useId,useState,type CSSProperties,type ReactNode} from 'react';
import {themeCatalog,themePalettes} from '../tokens/tokens.generated';
import {cx} from './controls';
export type BuiltinThemeId=keyof typeof themePalettes;
export {themeCatalog,themePalettes};
export function themeStyle(id:BuiltinThemeId):CSSProperties {const theme=themePalettes[id];return {...Object.fromEntries(Object.entries(theme.colors).map(([key,value])=>[`--${key}`,value])),'--terminal-background':theme.terminal.background,colorScheme:theme.mode} as CSSProperties;}
export function ThemeScope({id,children,className}:{id:BuiltinThemeId;children:ReactNode;className?:string}){return <div className={cx('sd-theme-scope',className)} data-theme={id} style={themeStyle(id)}>{children}</div>;}
export function ThemePreview({mode,themeId}:{mode?:'light'|'dark';themeId?:BuiltinThemeId}){
 const id=themeId??(mode==='light'?'soft-light':'soft-dark');
 return <span className="sd-theme-scope theme-preview" data-theme={id} style={themeStyle(id)} aria-hidden="true"><span className="preview-title"><i/><i/><i/></span><span className="preview-body"><span className="preview-rail"><i/><i/><i/></span><span className="preview-content"><b/><span/><span/><span className="preview-gradient"/></span></span></span>;
}
export function ThemeChoice({id,selected,onChange,disabled,busy,name,defaultTheme=false}:{id:BuiltinThemeId;selected:boolean;onChange():void;disabled?:boolean;busy?:boolean;name:string;defaultTheme?:boolean}){const inputId=useId(),theme=themeCatalog.themes.find(t=>t.id===id)!;return <label className="sd-theme-choice appearance-option" data-selected={selected} htmlFor={inputId}><input id={inputId} type="radio" name={name} value={id} checked={selected} disabled={disabled} aria-disabled={busy||undefined} onChange={()=>{if(!busy)onChange();}}/><ThemePreview themeId={id}/><span className="sd-theme-choice-label"><strong>{theme.label}</strong>{defaultTheme&&<small>默认</small>}{selected&&<span aria-hidden="true">✓</span>}</span></label>;}
const logo=new URL('../../../../assets/brand/pocket-deck.svg',import.meta.url).href;
const compact=new URL('../../../../assets/brand/pocket-deck-micro.svg',import.meta.url).href;
export function BrandMark({size='large',decorative=false}:{size?:'small'|'medium'|'large';decorative?:boolean}){const [failed,setFailed]=useState(false);return failed?<span className="sd-brand sd-brand-fallback" data-size={size} role={decorative?undefined:'img'} aria-label={decorative?undefined:'SoDeck'} aria-hidden={decorative||undefined}>&gt;_</span>:<img className="sd-brand" data-size={size} src={size==='small'?compact:logo} alt={decorative?'':'SoDeck'} aria-hidden={decorative||undefined} draggable={false} onError={()=>{console.warn('SoDeck brand: asset unavailable; using command mark');setFailed(true);}}/>;}
