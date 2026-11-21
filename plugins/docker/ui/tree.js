import {TreeLayout} from './tree-layout.js';
export const escape=value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const paths={
  engine:'M4 4h16v6H4zM4 14h16v6H4zM7 7h.01M7 17h.01',
  root:'M3 7h7l2 2h9v11H3zM3 7V4h7l2 3',
  container:'m12 3 9 5-9 5-9-5zM3 8v9l9 5 9-5V8M12 13v9',
  image:'m12 3 9 5-9 5-9-5zM3 12l9 5 9-5M3 16l9 5 9-5',
  network:'M9 3h6v6H9zM2 16h6v6H2zM16 16h6v6h-6zM12 9v4M5 16v-3h14v3',
  volume:'M20 5c0 2-4 3-8 3s-8-1-8-3 4-3 8-3 8 1 8 3ZM4 5v14c0 2 4 3 8 3s8-1 8-3V5M4 12c0 2 4 3 8 3s8-1 8-3',
  group:'m12 3 9 5-9 5-9-5zM3 12l9 5 9-5M3 16l9 5 9-5',
  reference:'M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2',
  arrow:'m9 5 7 7-7 7',more:'M4 12h.01M12 12h.01M20 12h.01',
  load:'M12 8v5M12 16h.01M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0',
  hint:'M12 8v5M12 16h.01M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0',
  loading:'M21 12a9 9 0 1 1-9-9',
};
export function icon(kind){return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${paths[kind]||paths.root}"/></svg>`;}
export class ResourceTree {
  constructor(viewport,handlers){
    this.viewport=viewport;this.root=viewport.querySelector('[role=tree]');this.handlers=handlers;this.page={nodes:[],offset:0,total:0};this.positions=new Map();this.layout=new TreeLayout();
    this.rows=document.createElement('div');this.rows.setAttribute('role','none');this.root.append(this.rows);
    this.inline=document.createElement('section');this.inline.id='tree-inline';this.inline.setAttribute('role','group');this.inline.setAttribute('aria-label','当前对象');this.inline.hidden=true;this.root.append(this.inline);
    this.region=new SoDeckDOM.DomRegion(this.rows);
    this.observer=new ResizeObserver(()=>{if(this.inline.hidden)return;const anchor=this.anchor();if(this.layout.measure(this.inline.getBoundingClientRect().height)){this.paint();this.restore(anchor);}});this.observer.observe(this.inline);
    this.rows.addEventListener('click',e=>{const row=e.target.closest('[data-tree-key]');if(!row)return;const n=this.node(row.dataset.treeKey);if(!n)return;const action=e.target.closest('[data-tree-action]')?.dataset.treeAction;if(action==='toggle')handlers.toggle(n);else if(action==='menu')handlers.menu(n,e.target.closest('button'));else handlers.select(n,true);});
    this.rows.addEventListener('contextmenu',e=>{const row=e.target.closest('[data-tree-key]');if(!row)return;e.preventDefault();const n=this.node(row.dataset.treeKey);handlers.select(n,false);handlers.menu(n,row);});
    this.rows.addEventListener('keydown',e=>this.keydown(e));
    viewport.addEventListener('scroll',()=>{handlers.window(Math.max(0,this.layout.index(viewport.scrollTop)-5));handlers.visibility?.(this.inlineVisible());},{passive:true});
  }
  node(key){if(!key)return null;return this.page.nodes.find(n=>n.key===key)||(this.page.inlineAnchor?.node?.key===key?this.page.inlineAnchor.node:null);}
  inlineVisible(){return !this.inline.hidden&&this.inline.offsetTop<this.viewport.scrollTop+this.viewport.clientHeight&&this.inline.offsetTop+this.inline.offsetHeight>this.viewport.scrollTop;}
  anchor(){const index=this.layout.index(this.viewport.scrollTop),n=this.page.nodes[index-this.page.offset]||(this.page.inlineAnchor?.index===index?this.page.inlineAnchor.node:null);return n?{key:n.key,index,offsetY:this.layout.offset(index)-this.viewport.scrollTop}:null;}
  restore(anchor){if(!anchor)return;const index=this.positions.get(anchor.key);if(index!==undefined)this.viewport.scrollTop=this.layout.offset(index)-anchor.offsetY;}
  render(page,selected,motion=false){const anchor=this.anchor();this.page=page||{nodes:[],offset:0,total:0};this.selected=selected;this.layout.reset(this.page.total,this.page.inlineAnchor);this.page.nodes.forEach((n,i)=>this.positions.set(n.key,this.page.offset+i));if(this.page.inlineAnchor)this.positions.set(this.page.inlineAnchor.node.key,this.page.inlineAnchor.index);this.paint(motion);this.restore(anchor);}
  paint(motion=false){
    const nodes=[...this.page.nodes];const extra=this.page.inlineAnchor?.node;if(extra&&!nodes.some(n=>n.key===extra.key))nodes.push(extra);
    const entry=nodes.some(n=>n.key===this.selected)?this.selected:nodes[0]?.key;
    this.region.update(this.rows,nodes.map(n=>{const kind=['load','hint'].includes(n.type)?n.type:n.resource?.kind||n.branchKind||n.type;const index=this.positions.get(n.key);return `<div role="treeitem" data-dom-key="${escape(n.key)}" data-tree-key="${escape(n.key)}" class="tree-row tree-${escape(n.type)} ${n.health==='unhealthy'?'tree-error':''}" tabindex="${entry===n.key?'0':'-1'}" aria-level="${n.depth+1}" aria-selected="${n.key===this.selected}" ${n.expandable?`aria-expanded="${!!n.expanded}"`:''} ${n.expanded&&['resource','tool'].includes(n.type)?'aria-controls="tree-inline" aria-owns="tree-inline"':''} style="--depth:${n.depth};top:${this.layout.offset(index)}px" title="${escape([n.label,n.meta].filter(Boolean).join(' · '))}"><button type="button" tabindex="-1" class="tree-toggle ${n.expanded?'expanded':''}" data-tree-action="toggle" aria-label="${n.expanded?'折叠':'展开'}${escape(n.label)}" ${n.expandable?'':'hidden'}>${icon('arrow')}</button><span class="tree-icon">${icon(kind)}</span><span class="tree-label">${escape(n.label)}</span>${n.loading?`<span class="tree-loading">${icon('loading')}</span>`:''}<span class="tree-meta">${escape(n.meta)}</span>${n.key===this.selected?`<button type="button" tabindex="-1" class="tree-more" data-tree-action="menu" aria-label="${escape(n.label)}的更多操作">${icon('more')}</button>`:''}</div>`;}).join(''),motion);
    this.root.style.height=this.layout.size+'px';this.inline.hidden=!this.page.inlineAnchor;
    if(this.page.inlineAnchor){this.inline.style.top=((this.page.inlineAnchor.index+1)*this.layout.rowHeight)+'px';this.inline.style.setProperty('--depth',this.page.inlineAnchor.node.depth);}
  }
  focus(key){const row=[...this.rows.querySelectorAll('[data-tree-key]')].find(r=>r.dataset.treeKey===key);row?.focus({preventScroll:true});if(row){const top=this.layout.offset(this.positions.get(key));if(top<this.viewport.scrollTop)this.viewport.scrollTop=top;else if(top+this.layout.rowHeight>this.viewport.scrollTop+this.viewport.clientHeight)this.viewport.scrollTop=top+this.layout.rowHeight-this.viewport.clientHeight;}return !!row;}
  keydown(e){if(e.target.matches('input,select,textarea'))return;const row=e.target.closest('[data-tree-key]');if(!row)return;const n=this.node(row.dataset.treeKey),index=this.positions.get(n.key);
    if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();this.handlers.focusIndex(Math.max(0,Math.min(this.page.total-1,index+(e.key==='ArrowDown'?1:-1))));}
    else if(e.key==='Home'||e.key==='End'){e.preventDefault();this.handlers.focusIndex(e.key==='Home'?0:this.page.total-1);}
    else if(e.key==='ArrowRight'){e.preventDefault();if(n.expandable&&!n.expanded)this.handlers.toggle(n);else if(n.type==='resource'||n.type==='tool')this.inline.querySelector('button,input,select')?.focus();else if(n.expandable)this.handlers.focusIndex(index+1,n.key);}
    else if(e.key==='ArrowLeft'){e.preventDefault();if(n.expandable&&n.expanded)this.handlers.toggle(n);else if(n.parent)this.handlers.focusKey(n.parent);}
    else if(e.key==='Enter'){e.preventDefault();this.handlers.open(n);}
    else if(e.key==='F10'&&e.shiftKey||e.key==='ContextMenu'){e.preventDefault();this.handlers.menu(n,row);}
  }
  dispose(){this.region.dispose();this.observer.disconnect();}
}
