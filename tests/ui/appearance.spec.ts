import {test,expect} from '@playwright/test';
import {presentationFixture,chooseMenu} from './presentation-fixture';
import {mkdir,writeFile} from 'node:fs/promises';

test.beforeEach(async({page})=>{
 await presentationFixture(page);
 await page.addInitScript(()=>{
  const calls:any[]=[];(window as any).__APPEARANCE_CALLS__=calls;
  (window as any).__TAURI_INTERNALS__={invoke:async(_command:string,{request}:any)=>{
   calls.push(request);const p=(window as any).__PRESENTATION__.handle(request);if(p)return p;
   const result=request.method==='servers.list'?{servers:[]}:request.method==='workbench.contributions'?{commands:[],views:[]}:request.method==='workbench.snapshot'?{schemaVersion:4,windowId:'main',windowRole:'main',workspaceIds:[],workspaces:[],views:[],dockSlots:[],revision:0,recoveryWarnings:[],pendingOpens:[],activeWorkspaceId:null,split:'none',splitRatio:.5}:{};
   return {requestId:request.requestId,result:['workbench.modal','windows.chrome'].includes(request.method)?null:result};
  }};
 });
 await page.goto('/');await expect(page.getByRole('heading',{name:'添加第一台服务器'})).toBeVisible();
});
async function open(page:import('@playwright/test').Page){await page.getByRole('button',{name:'应用菜单'}).click();await chooseMenu(page,'appearance');await expect(page.getByRole('radio',{name:'深色'})).toBeEnabled();}
test('selects complete themes immediately, same selection is idempotent and dialog restores focus',async({page})=>{
 await open(page);await page.getByRole('radio',{name:'浅色'}).click();await expect(page.locator('html')).toHaveAttribute('data-theme','soft-light');await expect(page.getByRole('radio',{name:'浅色'})).toBeChecked();
 await page.getByRole('radio',{name:'浅色'}).click();expect(await page.evaluate(()=>(window as any).__APPEARANCE_CALLS__.filter((r:any)=>r.method==='ui.theme.choose').length)).toBe(1);
 await page.screenshot({path:'.qa/t011/ui/appearance-light.png'});await page.keyboard.press('Escape');await expect(page.getByRole('dialog')).toHaveCount(0);await expect(page.getByRole('button',{name:'应用菜单'})).toBeFocused();
 await page.screenshot({path:'.qa/t011/ui/welcome-light.png'});await open(page);await page.getByRole('radio',{name:'深色'}).click();await expect(page.locator('html')).toHaveAttribute('data-theme','soft-dark');await page.screenshot({path:'.qa/t011/ui/appearance-dark.png'});
 await page.mouse.click(4,80);await expect(page.getByRole('dialog')).toHaveCount(0);await page.screenshot({path:'.qa/t011/ui/welcome-dark.png'});
});
test('failed persistence retains palette, external update converges and legacy notice remains visible',async({page})=>{
 await page.evaluate(()=>{(window as any).__PRESENTATION__.failure=true;(window as any).__PRESENTATION__.legacyPlugins=['sodeck.terminal'];});
 await open(page);await page.getByRole('radio',{name:'浅色'}).click();await expect(page.getByRole('alert')).toContainText('外观未更改');await expect(page.locator('html')).toHaveAttribute('data-theme','soft-dark');await expect(page.getByRole('radio',{name:'深色'})).toBeChecked();
 await expect(page.getByText('部分旧插件将在下次打开时应用外观。')).toBeVisible();await page.evaluate(()=>(window as any).__PRESENTATION__.change('light'));await expect(page.getByRole('radio',{name:'浅色'})).toBeChecked();
});
test('keyboard, enlarged text, small viewport, reduced motion and forced colors remain usable',async({page})=>{
 await page.setViewportSize({width:760,height:480});await page.emulateMedia({reducedMotion:'reduce'});await page.evaluate(()=>document.documentElement.style.fontSize='20px');await open(page);
 await page.getByRole('radio',{name:'深色'}).focus();await page.keyboard.press('ArrowLeft');await expect(page.getByRole('radio',{name:'浅色'})).toBeChecked();
 const bounds=await page.getByRole('dialog').boundingBox();expect(bounds!.x).toBeGreaterThanOrEqual(0);expect(bounds!.y+bounds!.height).toBeLessThanOrEqual(480);
 await page.emulateMedia({forcedColors:'active'});await page.screenshot({path:'.qa/t011/ui/appearance-accessible.png'});
 await page.keyboard.press('Escape');await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('rendered controls retain contrast in all three palettes and reduced transparency is solid',async({page})=>{
 const samples:any[]=[];
 async function contrast(selector:string,property='color',minimum=4.5){
  const sample=await page.locator(selector).first().evaluate((node,property)=>{
   const parse=(text:string)=>{const values=text.match(/[\d.]+/g)!.map(Number);return [...values.slice(0,3),values[3]??1];};
   const blend=(front:number[],back:number[])=>[0,1,2].map(i=>front[i]*front[3]+back[i]*(1-front[3]));
   const ancestry:Element[]=[];for(let el:Element|null=node;el;el=el.parentElement)ancestry.unshift(el);
   let background=[255,255,255];for(const el of ancestry){background=blend(parse(getComputedStyle(el).backgroundColor),background);if(el.matches('.sd-button'))background=blend(parse(getComputedStyle(el.querySelector('.sd-button-face')!).backgroundColor),background);}
   const foreground=blend(parse(getComputedStyle(node).getPropertyValue(property)),background);
   const luminance=(color:number[])=>color.map(c=>{c/=255;return c<=.04045?c/12.92:((c+.055)/1.055)**2.4;}).reduce((sum,c,i)=>sum+c*[.2126,.7152,.0722][i],0);
   const a=luminance(foreground),b=luminance(background);return {foreground,background,ratio:(Math.max(a,b)+.05)/(Math.min(a,b)+.05)};
  },property);
  samples.push({mode:await page.locator('html').getAttribute('data-theme'),selector,property,minimum,...sample});expect(sample.ratio,`${selector} ${property}`).toBeGreaterThanOrEqual(minimum);
 }
 for(const mode of ['dopamine','light','dark']){
  await page.evaluate(mode=>(window as any).__PRESENTATION__.change(mode),mode);await expect(page.locator('html')).toHaveAttribute('data-theme',mode==='dopamine'?mode:'soft-'+mode);
  await contrast('.statusbar');await contrast('.application-menu');await contrast('.welcome button.primary');
  await page.locator('.welcome button.primary').hover();await contrast('.welcome button.primary');
  await page.mouse.down();await contrast('.welcome button.primary');await page.mouse.move(1,1);await page.mouse.up();
  await open(page);await contrast('.dialog h2');await contrast('.dialog header p');await contrast('.appearance-help');await contrast('.sd-theme-choice-label strong');
  await page.locator('.appearance-option').first().hover();await contrast('.sd-theme-choice-label strong');await contrast('.appearance-option[data-selected=true]','border-top-color',3);
  await page.keyboard.press('Tab');await page.getByRole('radio',{name:'浅色'}).focus();await contrast('.appearance-option:has(input:focus-visible)','outline-color',3);
  await page.evaluate(()=>{(window as any).__PRESENTATION__.failure=true;});await page.getByRole('radio',{name:mode==='light'?'深色':'浅色'}).click();await expect(page.getByRole('alert')).toBeVisible();await contrast('.ui-notice.error');
  expect(await page.locator('.ui-notice.error').evaluate(el=>{const probe=document.createElement('span');probe.style.color='var(--error)';el.append(probe);const expected=getComputedStyle(probe).color;probe.remove();return getComputedStyle(el).color===expected;})).toBe(true);
  await page.keyboard.press('Escape');await page.evaluate(()=>(window as any).__PRESENTATION__.failure=false);
 }
 const cdp=await page.context().newCDPSession(page);await cdp.send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-transparency',value:'reduce'}]});await open(page);
 expect(await page.locator('.dialog-backdrop').evaluate(el=>getComputedStyle(el).backgroundColor)).toBe(await page.locator('html').evaluate(el=>getComputedStyle(el).backgroundColor));
 await mkdir('.qa/t011/ui',{recursive:true});await writeFile('.qa/t012/ui/contrast.json',JSON.stringify({samples,reducedTransparency:true},null,2));
});

test('flat dopamine has no decorative shadows and uses outer-edge contours',async({page})=>{
 await page.evaluate(()=>(window as any).__PRESENTATION__.change('dopamine'));
 await expect(page.locator('html')).toHaveAttribute('data-theme','dopamine');
 await open(page);
 for(const selector of ['.sd-dialog','.sd-button-face','.window-topbar']){
  const shadows=await page.locator(selector).evaluateAll(elements=>elements.map(el=>getComputedStyle(el).boxShadow));
  expect(shadows.every(value=>value==='none'||!/[1-9]/.test(value.replace(/rgba?\([^)]+\)/g,'')))).toBe(true);
 }
 expect(await page.locator('.sd-dialog').evaluate(el=>getComputedStyle(el).borderTopColor)).toBe('rgb(32, 33, 31)');
 await page.screenshot({path:'.qa/t012/ui/flat-appearance.png'});
});
