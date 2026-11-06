/* Shared by the shell and plugin frames; clipboard access stays in the host. */
(() => {
  window.installTextMenu = ({ read, write, notify }) => {
    let menu, source, version = 0, disposed = false;
    const mac = /mac/i.test(navigator.userAgentData?.platform || navigator.platform || '');
    function snapshot(element) {
      const field = element.closest('input,textarea');
      if (field && !field.disabled && !['button', 'submit', 'reset', 'checkbox', 'radio', 'file', 'hidden', 'range', 'color'].includes(field.type)) {
        const start = field.selectionStart, end = field.selectionEnd;
        return { element: field, field, start, end, value: field.value, text: field.type === 'password' ? '' : start === null ? field.value : field.value.slice(start, end), writable: !field.readOnly };
      }
      const selection = getSelection();
      if (!selection?.rangeCount || selection.isCollapsed || !selection.containsNode(element, true)) return;
      const range = selection.getRangeAt(0).cloneRange();
      return { element, range, text: selection.toString(), writable: false, focus: document.activeElement };
    }
    function restore(s) {
      if (!s.element.isConnected) return;
      if (s.field) {
        s.field.focus({ preventScroll: true });
        if (s.start !== null) s.field.setSelectionRange(s.start, s.end);
      } else {
        s.focus?.focus?.({ preventScroll: true });
        const selection = getSelection(); selection.removeAllRanges(); selection.addRange(s.range);
      }
    }
    function close(focus = false) {
      version++;
      if (menu) { if (menu.hasAttribute('popover')) menu.hidePopover(); menu.remove(); menu = undefined; }
      if (focus && source) restore(source);
      source = undefined;
    }
    function replace(s, text) {
      const field = s.field;
      // insertText preserves the browser's editing undo stack in text fields.
      if (document.execCommand('insertText', false, text)) return;
      if (s.start !== null) field.setRangeText(text, s.start, s.end, 'end');
      else if (field.type === 'number' && (!text.trim() || Number.isFinite(Number(text)))) field.value = text.trim();
      else throw new Error('此输入框不支持该文本，请使用键盘编辑');
      field.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertFromPaste', data: text }));
    }
    async function action(choice, s) {
      close(); restore(s);
      const revision = version, focus = document.activeElement;
      const valid = () => !disposed && version === revision && document.hasFocus() && s.element.isConnected && document.activeElement === focus && (!s.field || s.field.value === s.value);
      if (choice === 'all') {
        if (s.field) s.field.select();
        return;
      }
      try {
        if (choice === 'paste') { const text = await read(valid); if (text !== null && valid()) replace(s, text); }
        else { const result = await write(s.text, valid); if (result !== false && valid() && choice === 'cut') replace(s, ''); }
      } catch (error) { if (valid()) notify(String(error)); }
    }
    function show(s, x, y) {
      close(); source = s;
      menu = document.createElement('div'); menu.className = 'sd-menu sd-text-menu';
      menu.setAttribute('role', 'menu'); menu.setAttribute('aria-label', '文本操作');
      const modifier = mac ? '⌘' : 'Ctrl+';
      const rows = s.field ? [['剪切', 'cut', 'X', !s.writable || !s.text], ['复制', 'copy', 'C', !s.text], ['粘贴', 'paste', 'V', !s.writable], ['全选', 'all', 'A', !s.value]] : [['复制', 'copy', 'C', false]];
      for (const [label, choice, key, disabled] of rows) {
        const button = document.createElement('button'); button.type = 'button'; button.setAttribute('role', 'menuitem'); button.disabled = disabled;
        button.textContent = label;
        const hint = document.createElement('span'); hint.className = 'shortcut'; hint.textContent = modifier + key; button.append(hint);
        button.addEventListener('click', () => { void action(choice, s); }); menu.append(button);
      }
      // Menus opened inside a modal must also belong to that modal's DOM tree.
      (s.element.closest('dialog[open]') || document.body).append(menu);
      if (typeof menu.showPopover === 'function') { menu.setAttribute('popover', 'manual'); menu.showPopover(); }
      const rect = menu.getBoundingClientRect();
      menu.style.left = Math.max(4, Math.min(x, innerWidth - rect.width - 4)) + 'px';
      menu.style.top = Math.max(4, Math.min(y, innerHeight - rect.height - 4)) + 'px';
      menu.querySelector('button:not(:disabled)')?.focus({ preventScroll: true });
    }
    const context = event => {
      if (event.defaultPrevented || !(event.target instanceof Element) || event.target.closest('.xterm,[role="menu"]')) return;
      const s = snapshot(event.target); if (!s) return;
      event.preventDefault(); show(s, event.clientX, event.clientY);
    };
    const outside = event => { if (!menu?.contains(event.target)) close(); };
    const key = event => {
      if (menu && event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(true); return; }
      if (menu && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault(); event.stopPropagation();
        const buttons = [...menu.querySelectorAll('button:not(:disabled)')], index = buttons.indexOf(document.activeElement);
        buttons[event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowUp' ? buttons.length - 1 : 1)) % buttons.length]?.focus(); return;
      }
      if (menu && event.key === 'Tab') { close(true); return; }
      if (!menu) version++;
      if ((event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) && event.target instanceof Element && !event.target.closest('.xterm')) {
        const s = snapshot(event.target); if (!s) return;
        event.preventDefault(); event.stopPropagation(); const rect = s.element.getBoundingClientRect(); show(s, rect.left + 8, rect.bottom);
      }
    };
    const blur = () => close();
    document.addEventListener('contextmenu', context);
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('keydown', key, true);
    window.addEventListener('blur', blur);
    return { close, dispose() { disposed = true; close(); document.removeEventListener('contextmenu', context); document.removeEventListener('pointerdown', outside, true); document.removeEventListener('keydown', key, true); window.removeEventListener('blur', blur); } };
  };
})();
