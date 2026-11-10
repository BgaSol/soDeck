import * as monaco from 'monaco-editor/editor/editor.api.js';
import EditorWorker from 'monaco-editor/editor/editor.worker.js?worker';
import {detectLanguage,languages} from './languages';

const workers=new Set<Worker>();
(globalThis as typeof globalThis & {MonacoEnvironment:monaco.Environment}).MonacoEnvironment={getWorker(){const worker=new EditorWorker();workers.add(worker);return worker;}};
const LIMIT=1048576,CAP=1000,encoder=new TextEncoder();
export interface EditorState {version:1;anchor:number;active:number;topLine:number;topOffset:number;left:number;search:{open:boolean;query:string;matchCase:boolean};language:string}
interface Options {host:HTMLElement;footer:HTMLElement;path:string;text:string;bom:boolean;eol:string;state?:EditorState;cursor?:number;scroll?:number;notify:(message:string,error?:boolean)=>void;change:()=>void;position:()=>void;save:()=>void}
interface Preferences {fontSize:number;lineHeight:number;tabSize:number;wordWrap:boolean;minimap?:boolean}

export class FileEditor {
  readonly model:monaco.editor.ITextModel;
  readonly editor:monaco.editor.IStandaloneCodeEditor;
  private options:Options;
  private observer:ResizeObserver;
  private disposables:monaco.IDisposable[]=[];
  private decorations:monaco.editor.IEditorDecorationsCollection;
  private timer:ReturnType<typeof setTimeout>|undefined;
  private find:HTMLElement;
  private query:HTMLInputElement;
  private count:HTMLElement;
  private caseButton:HTMLButtonElement;
  private language:HTMLSelectElement;
  private location:HTMLElement;
  private matches:monaco.editor.FindMatch[]=[];
  private current:monaco.Range|null=null;
  private capped=false;
  private matchCase=false;
  private override='auto';
  private reverting=false;
  private visible=true;
  private disposed=false;
  private compositionChanged=false;
  private pendingState:EditorState|undefined;
  private preferences:Preferences={fontSize:14,lineHeight:1.5,tabSize:4,wordWrap:false,minimap:true};
  composing=false;
  constructor(options:Options){
    this.options=options;
    const {host,footer,text}=options;
    host.classList.add('code-editor');host.dataset.textMenu='custom';
    this.model=monaco.editor.createModel(text,detectLanguage(options.path,text));
    this.model.setEOL(monaco.editor.EndOfLineSequence.LF);
    this.model.updateOptions({insertSpaces:false,trimAutoWhitespace:false});
    this.editor=monaco.editor.create(host,{model:this.model,readOnly:true,ariaLabel:'远端文件编辑器',fontSize:14,lineHeight:21,tabIndex:0,lineNumbers:'on',glyphMargin:false,folding:false,contextmenu:false,wordBasedSuggestions:'off',quickSuggestions:false,suggestOnTriggerCharacters:false,parameterHints:{enabled:false},autoClosingBrackets:'never',autoClosingQuotes:'never',autoIndent:'none',formatOnPaste:false,formatOnType:false,detectIndentation:false,trimAutoWhitespace:false,renderValidationDecorations:'off',links:false,unicodeHighlight:{ambiguousCharacters:false,invisibleCharacters:false},stickyScroll:{enabled:false},occurrencesHighlight:'off',selectionHighlight:false,dragAndDrop:false,dropIntoEditor:{enabled:false},minimap:{enabled:true,size:'fit',renderCharacters:false,showSlider:'always',maxColumn:100},scrollbar:{verticalScrollbarSize:8,horizontalScrollbarSize:8},scrollBeyondLastLine:false,overviewRulerLanes:0,padding:{top:10,bottom:10},renderLineHighlight:'gutter',maxTokenizationLineLength:10000});
    this.decorations=this.editor.createDecorationsCollection();
    this.find=document.createElement('div');this.find.className='editor-find';this.find.dataset.textMenu='default';this.find.hidden=true;this.find.setAttribute('role','search');
    this.query=document.createElement('input');this.query.type='text';this.query.maxLength=1024;this.query.placeholder='查找当前文件';this.query.setAttribute('aria-label','查找当前文件');this.query.autocomplete='off';this.query.spellcheck=false;
    this.count=document.createElement('span');this.count.className='editor-find-count';this.count.setAttribute('role','status');
    this.caseButton=this.button('区分大小写','Aa',()=>{this.matchCase=!this.matchCase;this.caseButton.setAttribute('aria-pressed',String(this.matchCase));this.search();this.options.position();});this.caseButton.setAttribute('aria-pressed','false');
    this.find.append(this.query,this.caseButton,this.button('上一处','上一处',()=>this.navigate(-1)),this.button('下一处','下一处',()=>this.navigate(1)),this.button('关闭搜索','关闭',()=>this.closeSearch()),this.count);host.append(this.find);
    this.location=document.createElement('span');this.location.className='editor-location';
    this.language=document.createElement('select');this.language.className='editor-language sd-control';this.language.setAttribute('aria-label','文件语言');
    for(const [value,label]of Object.entries({auto:'自动识别',...languages})){const option=document.createElement('option');option.value=value;option.textContent=label;this.language.append(option);}
    footer.append(this.location,this.language);
    this.language.addEventListener('change',()=>{this.override=this.language.value;this.applyLanguage();this.options.position();});
    this.query.addEventListener('input',()=>{if(!this.composing){this.scheduleSearch();this.options.position();}});
    this.query.addEventListener('compositionstart',()=>{this.composing=true;clearTimeout(this.timer);});
    this.query.addEventListener('compositionend',()=>{this.composing=false;this.scheduleSearch();this.options.position();});
    this.disposables.push(this.model.onDidChangeContent(()=>{if(this.reverting)return;if(this.composing){this.compositionChanged=true;return;}this.acceptChange();}),this.editor.onDidChangeCursorSelection(()=>{this.updateLocation();this.options.position();}),this.editor.onDidScrollChange(()=>this.options.position()),this.editor.onDidCompositionStart(()=>{this.composing=true;this.compositionChanged=false;this.editor.pushUndoStop();}),this.editor.onDidCompositionEnd(()=>{this.composing=false;if(this.compositionChanged){this.compositionChanged=false;this.acceptChange();}}));
    host.addEventListener('keydown',this.key,true);host.addEventListener('contextmenu',this.menu);host.addEventListener('paste',this.paste,true);host.addEventListener('copy',this.copy,true);host.addEventListener('cut',this.cut,true);document.addEventListener('pointerdown',this.outside,true);window.addEventListener('blur',this.closeMenu);
    this.observer=new ResizeObserver(()=>this.layout());this.observer.observe(host);
    this.editor.setPosition(this.model.getPositionAt(options.cursor||0));this.editor.setScrollTop(options.scroll||0);
    if(options.state?.version===1){this.pendingState=options.state;this.restore(options.state);}
    this.updateLocation();this.applyLanguage();
    (host as HTMLElement & {sodeckEditor:FileEditor}).sodeckEditor=this;
  }
  private button(label:string,text:string,action:()=>void){const button=document.createElement('button');button.type='button';button.className='sd-control';button.setAttribute('aria-label',label);button.textContent=text;button.addEventListener('click',action);return button;}
  value(){return this.model.getValue(monaco.editor.EndOfLinePreference.LF);}
  bytes(text=this.value()){return encoder.encode((this.options.bom?'\uFEFF':'')+(this.options.eol==='\r\n'?text.replaceAll('\n','\r\n'):text));}
  private acceptChange(){
    if(this.bytes().length>LIMIT){this.reverting=true;this.model.undo();this.reverting=false;this.options.notify('修改后将超过 1 MiB，已取消本次修改',true);return;}
    // Keep an over-limit native input undoable without reverting preceding valid input.
    this.model.pushStackElement();this.options.change();this.applyLanguage();if(this.query.value)this.scheduleSearch();
  }
  setText(text:string){
    if(this.isReadOnly())return false;
    if(this.bytes(text).length>LIMIT){this.options.notify('修改后将超过 1 MiB，已取消本次修改',true);return false;}
    this.editor.pushUndoStop();this.editor.executeEdits('sodeck',[{range:this.model.getFullModelRange(),text}]);this.editor.pushUndoStop();return true;
  }
  selection(){const s=this.editor.getSelection()!;return {anchor:this.model.getOffsetAt(s.getSelectionStart()),active:this.model.getOffsetAt(s.getPosition())};}
  setSelection(anchor:number,active:number){const a=this.model.getPositionAt(anchor),b=this.model.getPositionAt(active);this.editor.setSelection(new monaco.Selection(a.lineNumber,a.column,b.lineNumber,b.column));}
  state():EditorState {if(this.pendingState)return this.pendingState;const range=this.editor.getVisibleRanges()[0];const topLine=range?.startLineNumber||1;return {version:1,...this.selection(),topLine,topOffset:this.editor.getScrollTop()-this.editor.getTopForLineNumber(topLine),left:this.editor.getScrollLeft(),search:{open:!this.find.hidden,query:this.query.value,matchCase:this.matchCase},language:this.override};}
  private restore(state:EditorState){this.setSelection(state.anchor,state.active);const line=Math.max(1,Math.min(this.model.getLineCount(),state.topLine));this.editor.setScrollPosition({scrollTop:this.editor.getTopForLineNumber(line)+state.topOffset,scrollLeft:state.left});this.override=state.language==='auto'||languages[state.language]?state.language:'auto';this.language.value=this.override;this.query.value=state.search.query.slice(0,1024);this.matchCase=state.search.matchCase;this.caseButton.setAttribute('aria-pressed',String(this.matchCase));if(state.search.open){this.find.hidden=false;this.search(false);}}
  private applyLanguage(){const language=this.override==='auto'?detectLanguage(this.options.path,this.value()):this.override;if(this.model.getLanguageId()!==language)monaco.editor.setModelLanguage(this.model,language);this.language.title='当前语言：'+languages[language];this.language.options[0].textContent='自动 · '+languages[language];}
  preferencesChanged(values:Preferences,family:string){const state=this.state();this.preferences=values;this.model.updateOptions({tabSize:values.tabSize});this.editor.updateOptions({fontFamily:family,fontSize:values.fontSize,lineHeight:Math.round(values.fontSize*values.lineHeight),wordWrap:values.wordWrap?'on':'off'});this.layout();this.restore(state);}
  theme(tokens:Record<string,string>){tokens=Object.fromEntries(Object.entries(tokens).map(([key,value])=>[key,/^#[0-9a-f]{3,4}$/i.test(value)?'#'+[...value.slice(1)].map(c=>c+c).join(''):value]));const dark=tokens['color-scheme']==='dark';monaco.editor.defineTheme('sodeck-files',{base:dark?'vs-dark':'vs',inherit:true,rules:[{token:'comment',foreground:dark?'9AAE9B':'526D55'},{token:'key',foreground:dark?'9CCBFF':'175C9E'},{token:'keyword',foreground:dark?'CFB7FF':'6840A3'},{token:'type',foreground:dark?'9CCBFF':'175C9E'},{token:'string',foreground:dark?'E2B990':'995126'},{token:'number',foreground:dark?'AAD8BB':'24633E'},{token:'variable',foreground:dark?'E5CE8C':'765500'}],colors:{'editor.background':tokens.panel||'#ffffff','editor.foreground':tokens.text||'#263044','editorLineNumber.foreground':tokens.muted||'#64748b','editorGutter.background':tokens.panel||'#ffffff','editorCursor.foreground':tokens.text||'#263044','editor.selectionBackground':dark?'#385372':'#C7DDF5','editor.inactiveSelectionBackground':dark?'#334155':'#DAE5F1','editorWidget.background':tokens.panel||'#ffffff','minimap.background':tokens.panel||'#ffffff'}});monaco.editor.setTheme('sodeck-files');}
  private layout(){if(this.disposed||!this.visible)return;const width=this.options.host.clientWidth;this.editor.updateOptions({minimap:{enabled:this.preferences.minimap!==false&&width>=360,size:'fit',renderCharacters:false,showSlider:'always',maxColumn:100},overviewRulerLanes:width<360?1:0});this.editor.layout();if(this.pendingState&&width>0&&this.options.host.clientHeight>0){const state=this.pendingState;this.pendingState=undefined;this.restore(state);}}
  visibility(visible:boolean){this.visible=visible;if(visible)this.layout();}
  readOnly(value:boolean){if(!value)this.layout();this.editor.updateOptions({readOnly:value});if(value)this.closeMenu();}
  isReadOnly(){return this.editor.getOption(monaco.editor.EditorOption.readOnly);}
  focus(){this.editor.focus();}
  private updateLocation(){const p=this.editor.getPosition();this.location.textContent=p?`行 ${p.lineNumber}，列 ${p.column}`:'';}
  openSearch(){const s=this.editor.getSelection();const selected=s?this.model.getValueInRange(s):'';if(selected&&!selected.includes('\n')&&selected.length<=256)this.query.value=selected;this.find.hidden=false;this.search();this.query.focus();this.query.select();this.options.position();}
  closeSearch(){clearTimeout(this.timer);this.find.hidden=true;this.decorations.clear();this.focus();this.options.position();}
  private scheduleSearch(){clearTimeout(this.timer);this.matches=[];this.current=null;this.decorations.clear();this.count.textContent=this.query.value?'查找中…':'';this.timer=setTimeout(()=>this.search(),120);}
  private search(reveal=true){
    clearTimeout(this.timer);const query=this.query.value;
    this.matches=query?this.model.findMatches(query,false,false,this.matchCase,null,false,CAP+1):[];
    this.capped=this.matches.length>CAP;this.matches=this.matches.slice(0,CAP);this.current=null;
    if(reveal&&this.matches.length){const start=this.editor.getSelection()!.getStartPosition();const next=this.model.findNextMatch(query,start,false,this.matchCase,null,false);if(next){this.current=next.range;this.editor.setSelection(next.range);this.editor.revealRangeInCenterIfOutsideViewport(next.range);}}
    this.renderMatches();
  }
  navigate(direction:number){if(!this.query.value)return;this.search(false);const p=this.editor.getSelection();if(!p)return;const start=direction>0?p.getEndPosition():p.getStartPosition();const match=direction>0?this.model.findNextMatch(this.query.value,start,false,this.matchCase,null,false):this.model.findPreviousMatch(this.query.value,start,false,this.matchCase,null,false);if(!match)return;this.current=match.range;this.editor.setSelection(match.range);this.editor.revealRangeInCenterIfOutsideViewport(match.range);this.renderMatches();const wrapped=direction>0?match.range.getStartPosition().isBefore(start):start.isBefore(match.range.getEndPosition());if(wrapped)this.count.textContent+=direction>0?' · 已回到开头':' · 已回到末尾';this.options.position();}
  private renderMatches(){const items=this.find.hidden?[]:this.matches.map(match=>({range:match.range,options:{className:'file-search-match',minimap:{color:'#B28B36',position:monaco.editor.MinimapPosition.Inline},overviewRuler:{color:'#B28B36',position:monaco.editor.OverviewRulerLane.Center}}}));if(this.current&&!this.find.hidden)items.push({range:this.current,options:{className:'file-search-current',minimap:{color:'#D06B18',position:monaco.editor.MinimapPosition.Inline},overviewRuler:{color:'#D06B18',position:monaco.editor.OverviewRulerLane.Center}}});this.decorations.set(items);this.count.textContent=!this.query.value?'':!this.matches.length?'没有匹配':this.capped?`1,000+ 处${this.current?' · 行 '+this.current.startLineNumber:''}`:`${this.current?this.matches.findIndex(m=>m.range.equalsRange(this.current!))+1+' / ':''}${this.matches.length} 处`;this.count.title=this.capped?'命中过多，仅标记部分位置；仍可继续查找全文':'';}
  private key=(event:KeyboardEvent)=>{if(this.contextMenu){if(event.key==='Escape'||event.key==='Tab'){event.preventDefault();event.stopImmediatePropagation();this.closeMenu();this.focus();return;}if(['ArrowUp','ArrowDown','Home','End'].includes(event.key)){event.preventDefault();event.stopImmediatePropagation();const buttons=[...this.contextMenu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')],index=buttons.indexOf(document.activeElement as HTMLButtonElement);buttons[event.key==='Home'?0:event.key==='End'?buttons.length-1:(index+(event.key==='ArrowUp'?buttons.length-1:1))%buttons.length]?.focus();return;}}if(event.isComposing||this.composing)return;const mod=event.ctrlKey||event.metaKey;let action:(()=>void)|undefined;if(mod&&event.key.toLowerCase()==='f')action=()=>this.openSearch();else if(mod&&event.key.toLowerCase()==='s')action=this.options.save;else if(event.key==='F3'||(event.target===this.query&&event.key==='Enter'))action=()=>this.navigate(event.shiftKey?-1:1);else if(event.key==='Escape'&&!this.find.hidden)action=()=>this.closeSearch();else if(event.target!==this.query&&mod&&!event.altKey&&['c','x','v'].includes(event.key.toLowerCase()))action=()=>void this.clipboard(event.key.toLowerCase());else if(event.key==='ContextMenu'||(event.shiftKey&&event.key==='F10'))action=()=>this.showMenu();if(action){event.preventDefault();event.stopImmediatePropagation();action();}};
  private paste=(event:ClipboardEvent)=>{if(this.find.contains(event.target as Node))return;event.preventDefault();event.stopImmediatePropagation();const text=event.clipboardData?.getData('text/plain');if(text!==undefined)this.insert(text);};
  private copy=(event:ClipboardEvent)=>{if(this.find.contains(event.target as Node))return;event.preventDefault();event.stopImmediatePropagation();void this.clipboard('c');};
  private cut=(event:ClipboardEvent)=>{if(this.find.contains(event.target as Node))return;event.preventDefault();event.stopImmediatePropagation();void this.clipboard('x');};
  private insert(text:string){if(this.editor.getOption(monaco.editor.EditorOption.readOnly))return;const range=this.editor.getSelection()!;text=text.replaceAll('\r\n','\n');if(text.includes('\0')||text.includes('\r')){this.options.notify('粘贴内容包含不支持的二进制或换行字符',true);return;}const value=this.value(),start=this.model.getOffsetAt(range.getStartPosition()),end=this.model.getOffsetAt(range.getEndPosition());if(this.bytes(value.slice(0,start)+text+value.slice(end)).length>LIMIT){this.options.notify('修改后将超过 1 MiB，已取消本次修改',true);return;}this.editor.pushUndoStop();this.editor.executeEdits('clipboard',[{range,text,forceMoveMarkers:true}]);this.editor.pushUndoStop();}
  private async clipboard(key:string){const bridge=(window as unknown as {sodeckClipboard?:{readText:(valid:()=>boolean)=>Promise<string|null>;writeText:(text:string,valid:()=>boolean)=>Promise<boolean>}}).sodeckClipboard;if(!bridge)return;const version=this.model.getVersionId(),selection=this.selection(),active=document.activeElement;const valid=()=>!this.disposed&&!this.isReadOnly()&&document.hasFocus()&&active===document.activeElement&&version===this.model.getVersionId()&&JSON.stringify(selection)===JSON.stringify(this.selection());try{if(key==='v'){if(this.editor.getOption(monaco.editor.EditorOption.readOnly))return;const text=await bridge.readText(valid);if(text!==null&&valid())this.insert(text);}else{const text=this.model.getValueInRange(this.editor.getSelection()!);if(!text)return;const success=await bridge.writeText(text,valid);if(success!==false&&valid()&&key==='x')this.insert('');}}catch(error){if(valid())this.options.notify(String(error),true);}}
  private contextMenu:HTMLElement|undefined;
  private outside=(event:PointerEvent)=>{if(!this.contextMenu?.contains(event.target as Node))this.closeMenu();};
  private menu=(event:MouseEvent)=>{if(this.find.contains(event.target as Node))return;event.preventDefault();event.stopPropagation();this.showMenu(event.clientX,event.clientY);};
  private closeMenu=()=>{this.contextMenu?.remove();this.contextMenu=undefined;};
  private showMenu(x?:number,y?:number){this.closeMenu();this.focus();const bounds=this.options.host.getBoundingClientRect();const menu=document.createElement('div');menu.className='sd-menu file-editor-menu';menu.setAttribute('role','menu');menu.setAttribute('aria-label','文本操作');for(const [label,key]of [['剪切','x'],['复制','c'],['粘贴','v'],['全选','all']]){const button=this.button(label,label,()=>{this.closeMenu();this.focus();if(key==='all')this.editor.setSelection(this.model.getFullModelRange());else void this.clipboard(key);});button.setAttribute('role','menuitem');button.disabled=(key==='x'||key==='v')&&this.editor.getOption(monaco.editor.EditorOption.readOnly);menu.append(button);}this.options.host.append(menu);this.contextMenu=menu;menu.style.left=Math.max(4,Math.min((x??bounds.left+40)-bounds.left,bounds.width-140))+'px';menu.style.top=Math.max(4,Math.min((y??bounds.top+40)-bounds.top,bounds.height-menu.offsetHeight-4))+'px';menu.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();}
  dispose(){if(this.disposed)return;this.disposed=true;clearTimeout(this.timer);this.observer.disconnect();this.closeMenu();document.removeEventListener('pointerdown',this.outside,true);window.removeEventListener('blur',this.closeMenu);this.options.host.removeEventListener('keydown',this.key,true);this.options.host.removeEventListener('contextmenu',this.menu);this.options.host.removeEventListener('paste',this.paste,true);this.options.host.removeEventListener('copy',this.copy,true);this.options.host.removeEventListener('cut',this.cut,true);this.disposables.forEach(d=>d.dispose());this.decorations.clear();this.editor.dispose();this.model.dispose();workers.forEach(worker=>worker.terminate());workers.clear();}
}
