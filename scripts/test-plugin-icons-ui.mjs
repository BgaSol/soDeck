import assert from 'node:assert/strict';
import fs from 'node:fs';
import {chromium} from 'playwright';
import {icon} from '../src/ui.ts';
import {createServer} from 'node:http';
const robot=fs.readFileSync('plugins/mcp/assets/icons/robot.svg');
const server=createServer((req,res)=>{if(req.url==='/'){res.setHeader('Content-Type','text/html');res.end('<html><body></body></html>');return;}res.setHeader('Content-Type','image/svg+xml');res.end(req.url==='/bad.svg'?'broken':robot);});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const base=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({headless:true});
try {
  const page=await browser.newPage();
  const source=fs.readFileSync('src/ui.ts','utf8');
  const {stripTypeScriptTypes}=await import('node:module');
  await page.goto(base);
  await page.setContent(`<style>.plugin-icon-image{opacity:0}.plugin-icon[data-ready="true"] .plugin-icon-image{opacity:1}.plugin-icon[data-ready="true"] .plugin-icon-fallback{display:none}svg{width:16px;height:16px}</style><main></main>`);
  await page.addScriptTag({type:'module',content:stripTypeScriptTypes(source,{mode:'transform'})+'\nobservePluginIcons();window.renderIcon=icon;'});
  await page.waitForFunction(()=>typeof window.renderIcon==='function');
  await page.evaluate(base=>document.querySelector('main').innerHTML=window.renderIcon({url:base+'/robot.svg'})+window.renderIcon({url:base+'/bad.svg'}),base);
  await page.waitForFunction(()=>document.querySelector('.plugin-icon')?.dataset.ready==='true');
  assert.equal(await page.locator('.plugin-icon').nth(1).getAttribute('data-ready'),null);
  for(const color of ['rgb(30, 31, 34)','rgb(223, 225, 229)','rgb(83, 70, 166)']) {
    await page.locator('main').evaluate((node,color)=>node.style.color=color,color);
    assert.equal(await page.locator('.plugin-icon-image').first().evaluate(node=>getComputedStyle(node).fill),color);
  }
  await page.evaluate(base=>document.querySelector('main').innerHTML=window.renderIcon({url:base+'/robot.svg'}),base);
  await page.waitForFunction(()=>document.querySelector('.plugin-icon')?.dataset.ready==='true');
  fs.mkdirSync('artifacts/plugin-icons',{recursive:true});await page.screenshot({path:'artifacts/plugin-icons/render.png'});
  assert(!icon({url:'test.svg'}).includes('<path d="M12 8'));
  console.log('Plugin SVG image masks load, theme color and preserve broken-image fallback across renders.');
}finally{await browser.close();await new Promise(r=>server.close(r));}
