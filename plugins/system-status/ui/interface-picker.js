export function createInterfacePicker(onSelect) {
  const $ = id => document.getElementById(id);
  const text = (id, value) => { if ($(id).textContent !== value) $(id).textContent = value; };
  const events = new AbortController(), {signal} = events;
  let selectedId;
  function interfaceRecord(node, item) {
    const name = Object.assign(document.createElement('strong'), { textContent: item?.name || item?.id || '暂不可用' });
    const addresses = Object.assign(document.createElement('span'), { className: 'interface-addresses', textContent: item?.removed ? '设备已移除' : item?.addresses?.length ? item.addresses.join(' · ') : item?.counterInstance ? '性能计数器实例，未映射物理网卡 · 未提供 IP' : '未提供 IP' });
    node.replaceChildren(name, addresses);
  }
  function interfaceOptions(items, selected) {
    selectedId = selected?.id;
    const menu = $('interface-options'), trigger = $('interface');
    const rows = selected?.removed ? [...items, selected] : items;
    const signature = JSON.stringify(rows);
    if (menu.dataset.items !== signature) {
      const focused = menu.contains(document.activeElement) ? document.activeElement.dataset.interfaceId : null;
      menu.replaceChildren();
      for (const item of rows) {
        const button = document.createElement('button');
        button.type = 'button'; button.dataset.interfaceId = item.id; button.disabled = Boolean(item.removed);
        button.setAttribute('role', 'option'); button.tabIndex = -1;
        const record = document.createElement('span'); record.className = 'interface-record'; interfaceRecord(record, item);
        button.append(record); menu.append(button);
      }
      menu.dataset.items = signature;
      if (focused) [...menu.children].find(button => button.dataset.interfaceId === focused)?.focus();
    }
    const focusTarget = [...menu.children].find(button => button.dataset.interfaceId === selected?.id && !button.disabled) || menu.querySelector('button:not(:disabled)');
    for (const button of menu.children) { button.setAttribute('aria-selected', String(button.dataset.interfaceId === selected?.id)); button.autofocus = button === focusTarget; }
    const current = rows.find(item => item.id === selected?.id) || selected;
    const currentSignature = JSON.stringify(current || null);
    if (trigger.dataset.current !== currentSignature) { interfaceRecord($('interface-current'), current); trigger.dataset.current = currentSignature; }
    trigger.disabled = !items.length;
    text('interface-count', items.length ? items.length + ' 个接口' : '');
    if (trigger.disabled) closeInterfaces();
    else if (menu.matches(':popover-open')) positionInterfaces();
  }
  function closeInterfaces(focus = false) {
    if ($('interface-options').matches(':popover-open')) $('interface-options').hidePopover();
    if (focus) $('interface').focus();
  }
  function positionInterfaces() {
    const menu = $('interface-options'), r = $('interface').getBoundingClientRect();
    const below = innerHeight - r.bottom - 8, above = r.top - 8;
    const down = below >= Math.min(320, menu.scrollHeight) || below >= above;
    menu.style.width = r.width + 'px'; menu.style.left = r.left + 'px';
    menu.style.maxHeight = Math.max(0, Math.min(320, down ? below : above)) + 'px';
    menu.style.top = (down ? r.bottom + 4 : r.top - menu.offsetHeight - 4) + 'px';
  }
  $('interface-options').addEventListener('beforetoggle', e => $('interface').setAttribute('aria-expanded', String(e.newState === 'open')), {signal});
  $('interface-options').addEventListener('toggle', e => { if (e.newState === 'open') positionInterfaces(); }, {signal});
  $('interface-options').addEventListener('click', e => {
    const button = e.target.closest('button'); if (!button || button.disabled) return;
    closeInterfaces(true);
    if (button.dataset.interfaceId !== selectedId) onSelect(button.dataset.interfaceId);
  }, {signal});
  $('interface-options').addEventListener('keydown', e => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeInterfaces(true); return; }
    if (e.key === 'Tab') { closeInterfaces(true); return; }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault();
    const buttons = [...$('interface-options').querySelectorAll('button:not(:disabled)')];
    const index = buttons.indexOf(document.activeElement);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (index + (e.key === 'ArrowUp' ? -1 : 1) + buttons.length) % buttons.length;
    buttons[next]?.focus();
  }, {signal});
  $('interface').addEventListener('keydown', e => {
    if (['ArrowDown', 'ArrowUp'].includes(e.key)) { e.preventDefault(); $('interface-options').showPopover(); }
  }, {signal});
  addEventListener('resize', () => { if ($('interface-options').matches(':popover-open')) positionInterfaces(); }, {signal});

  return {update: interfaceOptions, close: closeInterfaces, dispose() { closeInterfaces(); events.abort(); }};
}
