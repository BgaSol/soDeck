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
};
export function icon(kind){return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${paths[kind]||paths.root}"/></svg>`;}
export class ResourceTree {
  constructor(viewport,handlers){this.viewport=viewport;this.root=viewport.querySelector('[role=tree]');this.handlers=handlers;this.page={nodes:[],offset:0,total:0};this.positions=new Map();this.region=new SoDeckDOM.DomRegion(this.root);
    this.root.addEventListener('click',e=>{const row=e.target.closest('[data-tree-key]');if(!row)return;const n=this.node(row.dataset.treeKey);if(!n)return;const action=e.target.closest('[data-tree-action]')?.dataset.treeAction;if(action==='toggle')handlers.toggle(n);else if(action==='menu')handlers.menu(n,e.target.closest('button'));else handlers.select(n,true);});
    this.root.addEventListener('dblclick',e=>{if(e.target.closest('[data-tree-action]'))return;const row=e.target.closest('[data-tree-key]');if(row)handlers.open(this.node(row.dataset.treeKey));});
    this.root.addEventListener('contextmenu',e=>{const row=e.target.closest('[data-tree-key]');if(!row)return;e.preventDefault();const n=this.node(row.dataset.treeKey);handlers.select(n,false);handlers.menu(n,row);});
    this.root.addEventListener('keydown',e=>this.keydown(e));
    viewport.addEventListener('scroll',()=>handlers.window(Math.max(0,Math.floor(viewport.scrollTop/30)-5)),{passive:true});
  }
  node(key){return this.page.nodes.find(n=>n.key===key);}
  anchor(){const index=Math.floor(this.viewport.scrollTop/30),n=this.page.nodes[index-(this.page.offset||0)];return n?{key:n.key,index,offsetY:index*30-this.viewport.scrollTop}:null;}
  render(page,selected){const anchor=this.anchor();this.page=page||{nodes:[],offset:0,total:0};const nodes=this.page.nodes||[];const offset=this.page.offset||0;
    nodes.forEach((n,i)=>this.positions.set(n.key,offset+i));
    const entry=nodes.some(n=>n.key===selected)?selected:nodes[0]?.key;
    const html=`<div role="none" class="tree-space" style="height:${offset*30}px"></div>`+nodes.map(n=>{
      const kind=n.resource?.kind||n.branchKind||n.type;
      const root=n.type==='root',full=root?this.handlers.rootPath():n.meta;
      const label=[n.label,n.meta].filter(Boolean).join(' · ');
      return `<div role="treeitem" data-dom-key="${escape(n.key)}" data-tree-key="${escape(n.key)}" class="tree-row ${n.health==='unhealthy'?'tree-error':''}" tabindex="${entry===n.key?'0':'-1'}" aria-level="${n.depth+1}" aria-selected="${n.key===selected}" ${n.expandable?`aria-expanded="${!!n.expanded}"`:''} style="--depth:${n.depth}" title="${escape(root?full:label)}">
        <button type="button" tabindex="-1" class="tree-toggle ${n.expanded?'expanded':''}" data-tree-action="toggle" aria-label="${n.expanded?'折叠':'展开'}${escape(n.label)}" ${n.expandable?'':'hidden'}>${icon('arrow')}</button><span class="tree-icon">${icon(n.type==='reference'?'reference':kind)}</span><span class="tree-label">${escape(n.label)}</span><span class="tree-meta">${escape(n.meta)}</span>${n.key===selected?`<button type="button" tabindex="-1" class="tree-more" data-tree-action="menu" aria-label="${escape(n.label)}的更多操作">${icon('more')}</button>`:''}</div>`;
    }).join('')+`<div role="none" class="tree-space" style="height:${Math.max(0,(this.page.total-offset-nodes.length)*30)}px"></div>`;
    this.region.update(this.root,html);
    if(anchor){const at=nodes.findIndex(n=>n.key===anchor.key);if(at>=0)this.viewport.scrollTop=(offset+at)*30-anchor.offsetY;}
  }
  focus(key){const row=[...this.root.querySelectorAll('[data-tree-key]')].find(r=>r.dataset.treeKey===key);row?.focus({preventScroll:true});if(row){const index=this.positions.get(key);const top=index*30;if(top<this.viewport.scrollTop)this.viewport.scrollTop=top;else if(top+30>this.viewport.scrollTop+this.viewport.clientHeight)this.viewport.scrollTop=top+30-this.viewport.clientHeight;}return !!row;}
  keydown(e){const row=e.target.closest('[data-tree-key]');if(!row)return;const n=this.node(row.dataset.treeKey);if(!n)return;const index=this.positions.get(n.key);
    if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();this.handlers.focusIndex(Math.max(0,Math.min(this.page.total-1,index+(e.key==='ArrowDown'?1:-1))));}
    else if(e.key==='Home'||e.key==='End'){e.preventDefault();this.handlers.focusIndex(e.key==='Home'?0:this.page.total-1);}
    else if(e.key==='ArrowRight'){e.preventDefault();if(n.expandable&&!n.expanded)this.handlers.toggle(n);else if(n.expandable)this.handlers.focusIndex(index+1,n.key);}
    else if(e.key==='ArrowLeft'){e.preventDefault();if(n.expandable&&n.expanded)this.handlers.toggle(n);else if(n.parent)this.handlers.focusKey(n.parent);}
    else if(e.key==='Enter'){e.preventDefault();this.handlers.open(n);}
    else if(e.key==='F10'&&e.shiftKey){e.preventDefault();this.handlers.menu(n,row);}
    else if(e.key==='ContextMenu'){e.preventDefault();this.handlers.menu(n,row);}
  }
  dispose(){this.region.dispose();}
}
