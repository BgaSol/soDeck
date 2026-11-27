(() => {
  const e = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const paths = {
    play:'<path d="m8 5 11 7-11 7Z"/>', stop:'<rect x="6" y="6" width="12" height="12" rx="1"/>',
    edit:'<path d="m15 4 5 5M4 20l5-1L21 7a2 2 0 0 0-4-4L5 15Z"/>',
    delete:'<path d="M4 7h16M9 7V4h6v3M6 7l1 14h10l1-14M10 11v6m4-6v6"/>',
    copy:'<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V3H3v13h5"/>',
    check:'<path d="m5 12 4 4L19 6"/>',plus:'<path d="M12 5v14M5 12h14"/>',
    chevron:'<path d="m6 9 6 6 6-6"/>',left:'<path d="m15 6-6 6 6 6"/>',right:'<path d="m9 6 6 6-6 6"/>',
  };
  const icon = name => `<svg aria-hidden="true" viewBox="0 0 24 24" fill="${name==='play'||name==='stop'?'currentColor':'none'}" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">${paths[name]||''}</svg>`;
  const button = (action,label,id,disabled=false,name=action) => `<button type="button" class="sd-icon ${action==='start'||action==='stop'?'run':''} ${action==='delete'?'danger':''}" data-action="${action}" data-id="${e(id)}" ${action==='start'||action==='stop'?`data-dom-key="run:${e(id)}"`:''} aria-label="${e(label)}" title="${e(label)}" ${disabled?'disabled':''}>${icon(name)}</button>`;
  const address = a => a ? `${a.host.includes(':')?'['+a.host+']':a.host}:${a.port}` : '';
  const bytes = value => {const n=Number(value||0);return n<1024?`${n.toFixed(0)} B`:n<1048576?`${(n/1024).toFixed(1)} KiB`:`${(n/1048576).toFixed(1)} MiB`;};
  const time = value => value ? new Date(value).toLocaleString() : '尚无';
  const state = row => row.run?.queryError?'unknown':row.run?.stopRequested&&row.run?.canStart!==true?'stopping':row.run?.snapshot?.state||'closed';
  const names = {unknown:'状态待确认',starting:'正在启动',running:'本机入口已监听',stopping:'正在停止',closed:'已停止',failed:'运行失败'};
  const ready = row => state(row)==='running';
  const canStart = row => !row.run||row.run.canStart===true;
  const failure = (value,key) => !value?'':`<div class="failure"><p class="notice">${e(value.message)}</p>${value.hint?`<p class="muted">${e(value.hint)}</p>`:''}${value.diagnostic?`<details data-section="diagnostic:${e(key)}"><summary>诊断详情</summary><pre>${e(value.diagnostic)}</pre></details>`:''}</div>`;
  function detail(row,ui) {
    const r=row.rule,s=row.run?.snapshot||{},running=row.run?.rule||r,entry=address(s.address),check=ui.catalog.checks?.[r.id];
    const current=ready(row)&&check?.listenerId===s.listenerId?check:null,target=ui.targets.get(r.id)||{host:'',port:443};
    const checked=!current?'未检查':current.checking?'正在检查…':current.ok?'TCP 可达':'目标检查失败';
    return `<div class="detail" data-dom-key="detail:${e(r.id)}" id="detail-${e(r.id)}" data-dom-scope="detail:${e(r.id)}">
      <div class="entry"><span class="status ${e(state(row))}">${e(names[state(row)]||state(row))}</span><div class="entry-address"><code>${e(entry||'尚未监听')}</code>${button('copy',ui.copied.get(r.id)>Date.now()?'已复制':'复制',r.id,!ready(row),'copy')}</div><p class="muted route">${running.type==='socks5'?'SOCKS5 · 客户端指定目标':`远端目标：${e(address(running.target))}`}</p></div>
      ${row.run&&row.run.ruleRevision!==r.revision&&!canStart(row)?'<p class="muted">配置已保存。停止并确认完成后，再启动以应用。</p>':''}
      <dl class="stats"><div><dt>活动连接</dt><dd>${e(s.connections||0)}</dd></div><div><dt>上传</dt><dd>${row.run?.queryError?'已过期':bytes(s.sentPerSecond)+'/s'}</dd></div><div><dt>下载</dt><dd>${row.run?.queryError?'已过期':bytes(s.receivedPerSecond)+'/s'}</dd></div></dl>
      ${row.run?.queryError?`<p class="notice">${e(row.run.queryError)} · 最后观测 ${e(time(row.run.observedAt*1000))}。保留最后入口与累计量，当前状态待确认。</p>`:''}
      ${failure(s.failure,r.id+':listener')}
      <section class="check-result" aria-label="目标检查"><div class="check-line"><div><strong>${checked}</strong><p class="muted">${current?.observedAt?e(time(current.observedAt)):'通过实际本机入口检查 TCP'}</p></div><button type="button" class="sd-control" data-action="check" data-id="${e(r.id)}" ${!ready(row)||current?.checking||ui.busy.has(r.id)?'disabled':''}>${current?.checking?'正在检查…':'检查连接'}</button></div>
      ${r.type==='socks5'?`<div class="check-fields"><input id="check-host-${e(r.id)}" data-check="host" data-id="${e(r.id)}" aria-label="检查主机" placeholder="目标域名或 IP" maxlength="253" value="${e(target.host)}"><input id="check-port-${e(r.id)}" data-check="port" data-id="${e(r.id)}" aria-label="检查端口" type="number" min="1" max="65535" value="${e(target.port)}"></div>`:''}
      ${current?.target?`<p class="muted">检查目标：${e(address(current.target))}</p>`:''}${failure(current?.failure,r.id+':check')}${current?.ok?'<p class="muted">仅证明本次 TCP 可达，不代表应用健康。</p>':''}</section>
      ${row.run?.lastError?`<section class="client-failure"><strong>最近一次客户端失败</strong><p class="muted">${e(time(row.run.lastError.observedAt))}${row.run.lastError.target?' · '+e(address(row.run.lastError.target)):''}</p>${failure(row.run.lastError.failure,r.id+':client')}</section>`:''}
      <details data-section="path"><summary>连接路径</summary><p class="muted">${e(s.path?.nodes?.map(n=>`${n.kind==='ssh'?'SSH':'SOCKS5'} ${address(n)}`).join(' → ')||'通过当前服务器连接')}</p><p>本机 → 当前服务器的连接与跳板 → 目标服务</p></details>
      <details data-section="usage"><summary>累计流量与使用说明</summary><p class="muted">本次上传 ${bytes(s.sent)}，下载 ${bytes(s.received)}。</p>${r.type==='socks5'?`<p>客户端选择 SOCKS5，并启用代理端解析域名。</p><code>curl --proxy socks5h://${e(entry||'127.0.0.1:端口')} https://内网域名/</code><p class="muted">支持 TCP CONNECT；不包含 HTTP 代理和 UDP。</p>`:'<p>本地软件连接本机入口，即可访问远端目标。</p>'}</details>
      ${ui.feedback.get(r.id)?`<p class="feedback" role="status">${e(ui.feedback.get(r.id))}</p>`:''}
    </div>`;
  }
  function form(ui) {
    const d=ui.draft,fixed=d.type==='tcpForward',existing=d.revision>0;
    return `<form class="rule-form" data-dom-key="form:${e(d.id)}" data-dom-scope="form:${e(d.id)}" aria-label="${existing?'编辑':'新建'}规则"><fieldset ${ui.saving?'disabled':''}><legend>${existing?'编辑规则':'新建隧道'}</legend>
      <div class="type-choice"><button class="sd-control" type="button" data-type="tcpForward" aria-pressed="${fixed}" ${existing?'disabled':''}>TCP 转发</button><button class="sd-control" type="button" data-type="socks5" aria-pressed="${!fixed}" ${existing?'disabled':''}>SOCKS5 代理</button></div>
      <label class="field">名称 <small>可选</small><input name="name" maxlength="128" value="${e(d.name)}" placeholder="例如：生产数据库"></label>
      ${fixed?`<div class="target-fields"><label class="field">远端目标主机<input name="host" required maxlength="253" value="${e(d.target?.host)}" placeholder="db.internal"></label><label class="field">目标端口<input name="targetPort" required type="number" min="1" max="65535" value="${d.target?.port||''}" placeholder="3306"></label></div><p class="muted">从远端访问；127.0.0.1 指远端自身。</p>`:'<p class="muted">客户端指定目标，域名由远端解析。</p>'}
      <label class="field">本地端口<div class="inline"><input name="port" type="number" min="1" max="65535" ${d.bind.port===0?'disabled':''} value="${d.bind.port||''}" placeholder="自动分配"><label><input name="auto" type="checkbox" ${d.bind.port===0?'checked':''}> 自动</label></div></label>
      <details data-section="advanced"><summary>更多设置</summary><label class="field">本地绑定 IP<input name="bindHost" required value="${e(d.bind.host)}"></label><p class="muted">默认 127.0.0.1；也可指定其他本机 IP。</p></details><p class="muted">${existing?'保存不改变当前监听；停止后重新启动以应用。':'保存后手动启动，启动时显示实际入口。'}</p>
      <footer class="form-footer">${ui.cancelConfirm?'<span role="status">放弃未保存的修改？</span><button type="button" class="sd-quiet" data-action="keep-edit">继续编辑</button>':''}<button type="button" class="sd-control" data-action="cancel">${ui.cancelConfirm?'放弃修改':'取消'}</button><button class="sd-control sd-primary" type="submit">${ui.saving?'正在保存…':'保存'}</button></footer>
    </fieldset></form>`;
  }
  function manager(ui) {
    const {catalog,draft,selected}=ui,shown=draft&&(draft.revision===0||selected===draft.id&&catalog.rules.some(r=>r.rule.id===draft.id));
    return `<header class="toolbar"><h1>网络隧道</h1><button class="sd-quiet" type="button" data-action="create">${icon('plus')}新建</button></header><div class="counts"><span>${catalog.total||0} 条隧道</span><span>本页 ${catalog.rules.filter(ready).length} 条运行中</span></div>
      ${draft&&!shown?'<button class="sd-quiet resume-edit" type="button" data-action="resume-edit">继续编辑未完成的规则</button>':''}
      <div class="scroll" data-scroll="rules">${catalog.storageError?`<p class="notice storage-error">${e(catalog.storageError)}</p>`:''}${draft?.revision===0?form(ui):''}
      ${catalog.rules.map(row=>{const r=row.rule,editing=draft?.id===r.id,open=selected===r.id,blocked=ui.busy.has(r.id)||ui.saving||editing,can=canStart(row);return `<article class="rule ${open?'expanded':''}" data-dom-key="rule:${e(r.id)}" data-dom-scope="rule:${e(r.id)}" data-select="${e(r.id)}"><div class="rule-row"><button type="button" class="rule-toggle" data-action="expand" data-id="${e(r.id)}" aria-label="${e(row.name)}" aria-expanded="${open}" aria-controls="detail-${e(r.id)}"><span class="dot ${e(state(row))}"></span><span class="row-copy"><span class="row-title">${e(row.name)}<small>${r.type==='socks5'?'SOCKS5':'TCP'}</small></span><code>${e(ready(row)?address(row.run.snapshot.address):r.bind.port?address(r.bind):'自动分配端口')} · ${e(names[state(row)]||state(row))}</code></span><span class="chevron">${icon('chevron')}</span></button><div class="row-actions" role="group" aria-label="${e(row.name)}操作">${button(can?'start':'stop',can?'启动':'停止',r.id,blocked||state(row)==='stopping',can?'play':'stop')}${button('edit','编辑',r.id,ui.saving)}${button('delete',ui.deleteConfirm===r.id?'确认删除':'删除规则',r.id,!can||blocked,ui.deleteConfirm===r.id?'check':'delete')}</div></div>${open?(editing?`<div id="detail-${e(r.id)}">${form(ui)}</div>`:detail(row,ui)):''}</article>`;}).join('')}
      ${!catalog.total&&!draft?`<div class="empty">${icon('plus')}<h2>连接远端网络</h2><p class="muted">新建 TCP 转发或 SOCKS5 代理，<br>复用当前服务器的连接路径。</p><button class="sd-control" type="button" data-action="create">新建隧道</button></div>`:''}</div>
      <footer class="foot"><span>复用当前服务器连接</span><div>${button('previous','上一页规则','',!catalog.offset,'left')}<span>${Math.floor((catalog.offset||0)/20)+1} / ${Math.max(1,Math.ceil((catalog.total||0)/20))}</span>${button('next','下一页规则','',(catalog.offset||0)+20>=(catalog.total||0),'right')}</div></footer>`;
  }
  globalThis.NetworkPresentation={e,icon,address,bytes,state,ready,canStart,names,manager};
})();
