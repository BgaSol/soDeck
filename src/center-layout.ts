import type { CenterNode } from './components';

export const splitWidth = 6;
export function centerMinimum(node: CenterNode): {width:number;height:number} {
  if(node.type==='group')return {width:280,height:180};
  const a=centerMinimum(node.first),b=centerMinimum(node.second);
  return node.axis==='x'?{width:a.width+b.width+splitWidth,height:Math.max(a.height,b.height)}:{width:Math.max(a.width,b.width),height:a.height+b.height+splitWidth};
}
export function ratioBounds(node: Extract<CenterNode,{type:'split'}>,width:number,height:number) {
  const a=centerMinimum(node.first),b=centerMinimum(node.second),space=Math.max(1,(node.axis==='x'?width:height)-splitWidth);
  const min=Math.max(.05,(node.axis==='x'?a.width:a.height)/space),max=Math.min(.95,1-(node.axis==='x'?b.width:b.height)/space);
  return min<=max?{min,max}:{min:.5,max:.5};
}
export function effectiveRatio(node: Extract<CenterNode,{type:'split'}>,width:number,height:number,ratio=node.ratio) {
  const {min,max}=ratioBounds(node,width,height);return Math.max(min,Math.min(max,ratio));
}
export function splitNode(node:CenterNode,id:string):Extract<CenterNode,{type:'split'}>|undefined {
  if(node.type==='group')return;
  return node.id===id?node:splitNode(node.first,id)??splitNode(node.second,id);
}
export function withSplit(node:CenterNode,groupId:string,axis:'x'|'y'):CenterNode {
  if(node.type==='group')return node.groupId===groupId?{type:'split',id:'preview',axis,ratio:.5,first:node,second:{type:'group',groupId:'preview'}}:node;
  return {...node,first:withSplit(node.first,groupId,axis),second:withSplit(node.second,groupId,axis)};
}
