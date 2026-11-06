import assert from 'node:assert/strict';

// Clicking alone can scroll an incorrectly positioned menu into view.
export async function assertMenuLayout(page, trigger, {long=false}={}) {
  const popup=page.getByRole('menu');
  const geometry=await popup.evaluate(node=>{
    const rect=node.getBoundingClientRect(),list=node.querySelector('.sd-menu-list')??node,style=getComputedStyle(list);
    return {position:getComputedStyle(node).position,left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom,height:rect.height,horizontal:['auto','scroll'].includes(style.overflowX)&&list.scrollWidth>list.clientWidth,vertical:['auto','scroll'].includes(style.overflowY)&&list.scrollHeight>list.clientHeight};
  });
  assert.equal(geometry.position,'fixed',JSON.stringify(geometry));
  const {width,height}=page.viewportSize();
  assert(geometry.left>=7&&geometry.right<=width-7&&geometry.top>=7&&geometry.bottom<=height-7,JSON.stringify(geometry));
  assert(trigger,'The menu trigger must be visible before opening');
  assert(geometry.bottom<=trigger.y||geometry.top>=trigger.y+trigger.height,JSON.stringify({geometry,trigger}));
  const expectedLeft=Math.max(8,Math.min(trigger.x,width-(geometry.right-geometry.left)-8));
  assert(Math.abs(geometry.left-expectedLeft)<=1,JSON.stringify({geometry,trigger}));
  assert(Math.abs(geometry.top-trigger.y-trigger.height-4)<=1||Math.abs(geometry.bottom-trigger.y+4)<=1,JSON.stringify({geometry,trigger}));
  assert.equal(geometry.horizontal,false,JSON.stringify(geometry));
  assert.equal(geometry.vertical,long,JSON.stringify(geometry));
  return popup;
}
