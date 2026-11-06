// Exercise the plugin-generated policy, shared text menu and scrollbar styles.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const run = (command, args, options = {}) => {
  const result = spawnSync(command, args, { windowsHide: true, ...options });
  assert.equal(result.status, 0, result.error?.message || result.stderr || `${command} failed`);
  return result;
};
run('cargo', ['build', '--manifest-path', 'src-tauri/Cargo.toml', '--features', 'test-fixtures', '--bin', 'catalog-fixture', '--locked'], { stdio: 'inherit' });
const metadata = JSON.parse(run(path.resolve(`src-tauri/target/debug/catalog-fixture${process.platform === 'win32' ? '.exe' : ''}`), ['--desktop-script'], { encoding: 'utf8' }).stdout);
assert.equal(metadata.allFrames, true, 'The native plugin must inject its policy into child frames');
const policy = metadata.script;
fs.mkdirSync('artifacts/desktop-ui', { recursive: true });
const policyFile = path.resolve('artifacts/desktop-ui/defaults.js'); fs.writeFileSync(policyFile, policy);
const vite = await createServer({ server: { host: '127.0.0.1', port: 0 } }); await vite.listen();
const url = vite.resolvedUrls.local[0], browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ viewport: { width: 1100, height: 720 } });
await context.addInitScript({ content: policy });
const errors = [];
const html = frame => `<!doctype html><html data-theme="light"><head><meta charset="utf-8"></head><body>
<main style="padding:16px"><button id="first">前一项</button><input id="input" value="before"><button id="next">后一项</button>
<input id="password" type="password" value="secret"><input id="readonly" readonly value="read only"><input id="number" type="number" value="80">
<textarea id="editor">line one\nline two</textarea><p id="readable">copy this log</p><div id="business">业务菜单区域</div>
<div id="scroll" tabindex="0" aria-label="滚动内容" style="height:80px;width:230px;overflow:auto"><div style="height:360px;width:450px">滚动内容</div></div>
<dialog id="dialog"><input id="modal-input" value="modal"></dialog>${frame ? '' : '<iframe title="插件测试" src="/desktop-frame" style="width:700px;height:350px;border:0"></iframe>'}</main>
<script type="module">import '/ui.css';import '/themes.css';import '/text-menu.js';
window.clipboard='paste';window.copies=[];window.notifications=[];
window.textMenu=window.installTextMenu({read:async valid=>window.delayRead?new Promise(resolve=>window.finishRead=resolve):window.clipboard,write:async(text,valid)=>{if(valid())window.copies.push(text);return valid();},notify:e=>window.notifications.push(e)});
window.events=[];window.addEventListener('keydown',e=>window.events.push({key:e.key,prevented:e.defaultPrevented}));
window.addEventListener('contextmenu',e=>window.contextPrevented=e.defaultPrevented);
window.addEventListener('wheel',e=>window.lastWheel={target:e.target.id,delta:e.deltaY,x:e.clientX,y:e.clientY,prevented:e.defaultPrevented},{passive:true});
window.addEventListener('pointerdown',e=>window.lastPointer={target:e.target.id,x:e.clientX,y:e.clientY});
document.querySelector('#business').addEventListener('contextmenu',e=>{e.preventDefault();window.businessOpened=(window.businessOpened||0)+1;});window.ready=true;
</script></body></html>`;
await context.route(url, route => route.fulfill({ contentType: 'text/html', body: html(false) }));
await context.route(url + 'desktop-frame', route => route.fulfill({ contentType: 'text/html', body: html(true) }));
const page = await context.newPage(); page.on('pageerror', error => errors.push(String(error)));
const open = async locator => locator.dispatchEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 100, clientY: 100 });
try {
  await page.goto(url); await page.waitForFunction(() => window.ready);
  const child = page.frames().find(frame => frame.url().endsWith('/desktop-frame')); await child.waitForFunction(() => window.ready);
  for (const surface of [page, child]) {
    const input = surface.locator('#input');
    for (const key of ['F3', 'Shift+F3', 'F5', 'Control+r', 'Control+Shift+r', 'Control+f', 'Control+p', 'Control+o', 'Control+u', 'Control+j', 'Control+s', 'F12', 'Control+Shift+i', 'Control+Shift+j', 'Control+Shift+c', 'Control+Shift+=', 'Control+-', 'Control+0', 'Alt+ArrowLeft', 'Alt+ArrowRight', 'Meta+r', 'Meta+f', 'Meta+p', 'Meta+s', 'Meta+Alt+i', 'Meta+Shift+i', 'Meta+[', 'Meta+]']) {
      await input.focus(); await page.keyboard.press(key);
      assert.equal(await surface.evaluate(() => window.events.at(-1)?.prevented), true, `${surface.url()}: cancel browser default for ${key}`);
    }
    await surface.locator('#next').focus(); await page.keyboard.press('Shift+Tab');
    assert.equal(await input.evaluate(node => node === document.activeElement), true);
    await input.focus(); await page.keyboard.press('Control+a'); await page.keyboard.type('keyboard'); assert.equal(await input.inputValue(), 'keyboard');
    await input.fill('before'); await input.evaluate(node => node.setSelectionRange(0, 6)); await open(input);
    await surface.getByRole('menuitem', { name: /^复制/ }).click(); assert.equal(await surface.evaluate(() => window.copies.at(-1)), 'before');
    await open(input); await surface.getByRole('menuitem', { name: /^剪切/ }).click(); assert.equal(await input.inputValue(), '');
    await page.keyboard.press('Control+z'); assert.equal(await input.inputValue(), 'before', 'Context cut retains editing undo');
    await input.evaluate(node => node.setSelectionRange(0, 6)); await open(input); await surface.getByRole('menuitem', { name: /^粘贴/ }).click(); assert.equal(await input.inputValue(), 'paste');
    await page.keyboard.press('Control+z'); assert.equal(await input.inputValue(), 'before', 'Context paste retains editing undo');
    await surface.locator('#password').evaluate(node => node.select()); await open(surface.locator('#password'));
    assert(await surface.getByRole('menuitem', { name: /^复制/ }).isDisabled()); assert(await surface.getByRole('menuitem', { name: /^剪切/ }).isDisabled()); await page.keyboard.press('Escape');
    await open(surface.locator('#readonly')); assert(await surface.getByRole('menuitem', { name: /^粘贴/ }).isDisabled()); await page.keyboard.press('Escape');
    await surface.evaluate(() => { const range = document.createRange(); range.selectNodeContents(document.querySelector('#readable')); getSelection().removeAllRanges(); getSelection().addRange(range); });
    await open(surface.locator('#readable')); await surface.getByRole('menuitem', { name: /^复制/ }).click(); assert.equal(await surface.evaluate(() => window.copies.at(-1)), 'copy this log');
    await surface.evaluate(() => { window.delayRead = true; }); await open(input); await surface.getByRole('menuitem', { name: /^粘贴/ }).click();
    await surface.waitForFunction(() => window.finishRead); await surface.locator('#next').click(); await surface.evaluate(() => { window.finishRead('late'); window.delayRead = false; });
    assert.equal(await input.inputValue(), 'before', 'Late clipboard reads cannot edit a field after focus changes');
    await input.focus(); await page.keyboard.press('Shift+F10'); await surface.getByRole('menu', { name: '文本操作' }).waitFor(); await page.keyboard.press('Escape'); assert(await input.evaluate(node => node === document.activeElement));
    await surface.locator('#business').click({ button: 'right' }); assert.equal(await surface.evaluate(() => window.businessOpened), 1); assert.equal(await surface.evaluate(() => window.contextPrevented), true);
  }
  await page.evaluate(() => document.querySelector('#dialog').showModal());
  await page.locator('#modal-input').evaluate(node => node.setSelectionRange(5, 5));
  await open(page.locator('#modal-input')); assert.equal(await page.locator('dialog .sd-text-menu').count(), 1);
  await page.getByRole('menuitem', { name: /^粘贴/ }).click(); assert.equal(await page.locator('#modal-input').inputValue(), 'modalpaste'); await page.evaluate(() => document.querySelector('#dialog').close());
  for (const popover of [true, false]) {
    await page.setViewportSize({width:320,height:240});
    await page.evaluate(popover => {
      window.savedPopover=Object.getOwnPropertyDescriptor(HTMLElement.prototype,'showPopover');
      if(!popover)Object.defineProperty(HTMLElement.prototype,'showPopover',{configurable:true,value:undefined});
      const dialog=document.querySelector('#dialog');dialog.style.transform='translateY(0)';dialog.showModal();
      document.querySelector('#modal-input').value='selected';document.querySelector('#modal-input').setSelectionRange(1,5);
    },popover);
    await page.locator('#modal-input').focus();await page.keyboard.press('Shift+F10');
    const menu=page.locator('dialog .sd-text-menu');await menu.waitFor();
    const r=await menu.boundingBox();assert.ok(r.x>=7&&r.y>=7&&r.x+r.width<=313&&r.y+r.height<=233,JSON.stringify({popover,r,details:await menu.evaluate(node=>({style:node.style.cssText,placement:node.dataset.placement,dialog:node.parentElement.getBoundingClientRect().toJSON(),topLayer:node.matches(':popover-open'),input:document.querySelector('#modal-input').getBoundingClientRect().toJSON(),visual:visualViewport.height}))}));
    assert.equal(await menu.evaluate(node=>node.hasAttribute('popover')),popover);
    await page.keyboard.press('Escape');assert.deepEqual(await page.locator('#modal-input').evaluate(node=>[node===document.activeElement,node.selectionStart,node.selectionEnd]),[true,1,5]);
    await open(page.locator('#modal-input'));await page.getByRole('menuitem',{name:/^复制/}).click();assert.equal(await page.evaluate(()=>copies.at(-1)),'elec');
    await page.evaluate(()=>{document.querySelector('#dialog').close();document.querySelector('#dialog').style.transform='';if(savedPopover)Object.defineProperty(HTMLElement.prototype,'showPopover',savedPopover);});
  }
  await page.setViewportSize({width:1100,height:720});
  // Re-enter the plugin through an editable field, as in a normal focus handoff.
  await child.locator('#input').click(); assert(await child.locator('#input').evaluate(node => node === document.activeElement));
  for (const [theme, color] of [['light', 'rgb(167, 173, 187)'], ['dark', 'rgb(89, 101, 124)'], ['dopamine', 'rgb(146, 144, 123)']]) {
    for (const surface of [page, child]) {
      await surface.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
      const styles = await surface.locator('#scroll').evaluate(node => ({ width: getComputedStyle(node, '::-webkit-scrollbar').width, color: getComputedStyle(node, '::-webkit-scrollbar-thumb').backgroundColor, radius: getComputedStyle(node, '::-webkit-scrollbar-thumb').borderRadius }));
      assert.deepEqual(styles, { width: '10px', color, radius: '10px' });
      await surface.locator('#scroll').evaluate(node => node.scrollTop = 0);
      await surface.locator('#scroll').click({ position: { x: 30, y: 30 } });
      assert(await surface.locator('#scroll').evaluate(node => node === document.activeElement), 'The scroll region receives focus after closing a modal');
      await page.mouse.wheel(0, 500); await surface.waitForFunction(() => document.querySelector('#scroll').scrollTop > 0, {}, { timeout: 3000 });
      await surface.locator('#scroll').evaluate(node => node.scrollTop = 0);
      await page.keyboard.press('PageDown'); await surface.waitForFunction(() => document.querySelector('#scroll').scrollTop > 0, {}, { timeout: 3000 });
      await surface.locator('#scroll').evaluate(node => node.scrollLeft = 120); assert.equal(await surface.locator('#scroll').evaluate(node => node.scrollLeft), 120);
    }
    await page.screenshot({ path: `artifacts/desktop-ui/${theme}.png` });
  }
  await page.emulateMedia({ forcedColors: 'active' }); assert.equal(await page.locator('#scroll').evaluate(node => getComputedStyle(node).scrollbarColor), 'auto');
  assert.deepEqual(errors, []); assert.deepEqual(await page.evaluate(() => window.notifications), []);
  console.log('Desktop policy passed: browser defaults, plugin frame, Shift+Tab, editing/undo, local menus, delayed clipboard, modal and three-theme native scrollbars.');
} catch (error) {
  for (const surface of [page, ...page.frames().filter(frame => frame !== page.mainFrame())]) console.error(await surface.locator('#scroll').evaluate(node => ({ url: location.href, wheel:window.lastWheel,pointer:window.lastPointer,scale:visualViewport.scale,active:document.activeElement.tagName,scroll: node.scrollTop, height: node.clientHeight, content: node.scrollHeight, bounds: node.getBoundingClientRect().toJSON() })));
  await page.screenshot({ path: 'artifacts/desktop-ui/failure.png' }); throw error;
}
finally { await browser.close(); await vite.close(); }
run(process.execPath, ['scripts/test-terminal-input.mjs', '--desktop-script', policyFile], { stdio: 'inherit' });
