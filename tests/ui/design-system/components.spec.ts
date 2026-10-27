import {test,expect} from '@playwright/test';
test('three complete scopes preserve geometry, control states and isolated previews',async({page})=>{
 await page.goto('/design-system.html');const button=page.getByRole('button',{name:'primary',exact:true});let before;
 for(const label of ['多巴胺 默认','浅色','深色']){await page.getByRole('radio',{name:label}).check();await button.scrollIntoViewIfNeeded();const box=await button.boundingBox();if(before){expect(box!.width).toBeCloseTo(before.width,1);expect(box!.height).toBeCloseTo(before.height,1);}before=box;
 await expect(page.getByRole('button',{name:'不可用',exact:true})).toBeDisabled();await page.getByRole('button',{name:'处理中'}).dispatchEvent('click');await expect(page.getByText('已执行 0')).toBeVisible();
 expect(await page.locator('.theme-preview').evaluateAll(nodes=>nodes.map(n=>getComputedStyle(n).getPropertyValue('--sd-border-width').trim()))).toEqual(['2px','1px','1px']);
 await page.screenshot({path:'.qa/t012/ui/components-'+label.split(' ')[0]+'.png',fullPage:true});}
});
test('menu keyboard skips disabled, executes once and closes with Tab',async({page})=>{await page.goto('/design-system.html');await page.getByRole('menuitem',{name:/外观/}).focus();await page.keyboard.press('ArrowDown');await expect(page.getByRole('menuitem',{name:/重试保存/})).toBeFocused();await page.keyboard.press('End');await expect(page.getByRole('menuitem',{name:'退出 SoDeck'})).toBeFocused();await page.keyboard.press('Enter');await expect(page.getByText('菜单结果 exit')).toBeVisible();await page.keyboard.press('Tab');await expect(page.getByText('菜单结果 tab')).toBeVisible();});
test('dialog traps focus, ignores IME escape, restores trigger; reduced motion keeps feedback',async({page})=>{await page.goto('/design-system.html');await page.emulateMedia({reducedMotion:'reduce'});const button=page.getByRole('button',{name:'打开弹窗'});await button.click();await expect(page.getByRole('dialog')).toBeVisible();await page.getByLabel('弹窗输入').dispatchEvent('keydown',{key:'Escape',isComposing:true});await expect(page.getByRole('dialog')).toBeVisible();await page.keyboard.press('Escape');await expect(button).toBeFocused();const primary=page.getByRole('button',{name:'primary',exact:true});await primary.scrollIntoViewIfNeeded();await primary.hover();await page.mouse.down();expect(await primary.locator('.sd-button-content').evaluate(el=>getComputedStyle(el).translate)).toBe('none');await page.mouse.up();});

test('nested soft scopes do not inherit dopamine frame overrides',async({page})=>{
 await page.goto('/design-system.html');await page.getByRole('radio',{name:'多巴胺 默认'}).check();
 const styles=await page.evaluate(()=>{const scope=document.createElement('div');scope.className='sd-theme-scope';scope.dataset.theme='soft-dark';scope.setAttribute('style',document.querySelector('[data-theme="soft-dark"].theme-preview')!.getAttribute('style')!);scope.innerHTML='<input class="sd-input" aria-label="Scoped probe" />';document.querySelector('.catalog')!.append(scope);const shadow=getComputedStyle(scope.firstElementChild!).boxShadow;scope.remove();return shadow;});
 expect(styles).toContain('inset');
});
