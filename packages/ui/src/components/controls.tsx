import {useId, type ComponentPropsWithRef, type ButtonHTMLAttributes, type ComponentType, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type SVGProps} from 'react';

export const cx=(...parts:(string|false|undefined)[])=>parts.filter(Boolean).join(' ');
export type IconType=ComponentType<SVGProps<SVGSVGElement> & {size?:number|string;strokeWidth?:number|string}>;
export function Icon({icon:Glyph,className}:{icon:IconType;className?:string}){return <Glyph className={cx('sd-icon',className)} aria-hidden="true" focusable="false"/>;}
export type ButtonProps=Omit<ComponentPropsWithRef<'button'>,'style'> & {variant?:'primary'|'secondary'|'quiet'|'danger';size?:'normal'|'compact';busy?:boolean;icon?:IconType};
export function Button({variant='secondary',size='normal',busy=false,icon,className,children,onClick,type='button',...rest}:ButtonProps){
  return <button {...rest} type={type} className={cx('sd-button',className)} data-variant={variant} data-size={size} aria-busy={busy||undefined} aria-disabled={rest.disabled||busy||rest['aria-disabled']} onClick={e=>{if(busy||rest.disabled||rest['aria-disabled']===true||rest['aria-disabled']==='true'){e.preventDefault();return;}onClick?.(e);}}>
    <span className="sd-button-face" aria-hidden="true"/><span className="sd-button-content">{icon&&<Icon icon={icon}/>}<span className="sd-button-label">{children}</span>{busy&&<span className="sd-busy" aria-hidden="true"/>}</span>
  </button>;
}
export function IconButton({label,icon,children,...props}:Omit<ButtonProps,'icon'> & {label:string;icon?:IconType}){return <Button {...props} icon={icon} className={cx('sd-icon-button',props.className)} title={props.title??label} aria-label={label}>{children}</Button>;}
export function ToggleButton({pressed,...props}:ButtonProps & {pressed:boolean}){return <Button {...props} aria-pressed={pressed}/>;}
export function WindowControlButton({label,close=false,className,...props}:Omit<ButtonHTMLAttributes<HTMLButtonElement>,'style'> & {label:string;close?:boolean}) {
  return <button {...props} type="button" aria-label={label} title={props.title??label} data-close={close||undefined} className={cx('sd-window-control',className)}/>;
}
export function BusyIndicator({label='正在加载'}:{label?:string}) {return <span className="sd-busy-indicator" role="status" aria-label={label}><span className="sd-busy" aria-hidden="true"/></span>;}
export function InputGroup({children,className}:{children:ReactNode;className?:string}) {return <div className={cx('sd-input-group',className)}>{children}</div>;}
export function FormFields({className,...props}:import('react').FieldsetHTMLAttributes<HTMLFieldSetElement>){return <fieldset {...props} className={cx('sd-form-fields',className)}/>;}

type FieldInfo={label:ReactNode;labelVisibility?:'visible'|'sr-only';help?:ReactNode;error?:ReactNode};
/** For composition with an existing associated label or Field. */
export function Input({className,type,...props}:Omit<ComponentPropsWithRef<'input'>,'style'>){return <input {...props} type={type} className={cx(type==='checkbox'||type==='radio'?'sd-choice-input':'sd-input',className)}/>;}
export function Select({className,...props}:Omit<ComponentPropsWithRef<'select'>,'style'>){return <select {...props} className={cx('sd-input',className)}/>;}
export function Field({label,labelVisibility='visible',help,error,id,children,className}:{id:string;children:ReactNode;className?:string}&FieldInfo){return <div className={cx('sd-field',className)}><label className={labelVisibility==='sr-only'?'sd-sr-only':undefined} htmlFor={id}>{label}</label>{children}{help&&<span id={`${id}-help`} className="sd-field-help">{help}</span>}{error&&<span id={`${id}-error`} className="sd-field-error" role="alert">{error}</span>}</div>;}
function description(id:string,help:ReactNode,error:ReactNode,extra?:string){return [extra,help?`${id}-help`:undefined,error?`${id}-error`:undefined].filter(Boolean).join(' ')||undefined;}
export function TextField({label,labelVisibility,help,error,className,id:provided,...props}:Omit<InputHTMLAttributes<HTMLInputElement>,'style'> & FieldInfo){const generated=useId(),id=provided??generated;return <Field {...{label,labelVisibility,help,error,id,className}}><input {...props} id={id} className="sd-input" aria-invalid={error?true:props['aria-invalid']} aria-describedby={description(id,help,error,props['aria-describedby'])}/></Field>;}
export function SelectField({label,labelVisibility,help,error,className,id:provided,children,...props}:Omit<SelectHTMLAttributes<HTMLSelectElement>,'style'> & FieldInfo){const generated=useId(),id=provided??generated;return <Field {...{label,labelVisibility,help,error,id,className}}><select {...props} id={id} className="sd-input" aria-invalid={error?true:props['aria-invalid']} aria-describedby={description(id,help,error,props['aria-describedby'])}>{children}</select></Field>;}
export function Checkbox({label,className,...props}:Omit<InputHTMLAttributes<HTMLInputElement>,'style'|'type'> & {label:ReactNode}){return <label className={cx('sd-check',className)}><input {...props} type="checkbox"/>{label}</label>;}
export function RadioGroup({label,name,value,options,onChange,disabled=false}:{label:string;name?:string;value:string;options:readonly {value:string;label:ReactNode}[];onChange(value:string):void;disabled?:boolean}){const generated=useId();return <fieldset className="sd-radio-group" disabled={disabled}><legend>{label}</legend>{options.map(option=><label className="sd-check" key={option.value}><input type="radio" name={name??generated} value={option.value} checked={value===option.value} onChange={()=>onChange(option.value)}/>{option.label}</label>)}</fieldset>;}
