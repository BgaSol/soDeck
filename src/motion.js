// Display effects never delay RPC, focus changes or resource disposal.
const easing = 'cubic-bezier(0.16, 1, 0.3, 1)';
const stateKey = Symbol.for('sodeck.motion.observed');
export function motionDuration(element, kind = 'enter') {
  const document = element.ownerDocument, view = document.defaultView;
  observeMotion(document);
  if (!view || document.hidden || !document[stateKey].visible || view.matchMedia('(prefers-reduced-motion: reduce)').matches) return 0;
  const value = parseFloat(view.getComputedStyle(element).getPropertyValue(`--sd-motion-${kind}`));
  return Number.isFinite(value) ? Math.max(0, Math.min(300, value)) : kind === 'layout' ? 200 : kind === 'feedback' || kind === 'exit' ? 100 : 160;
}
export function observeMotion(document) {
  if (document[stateKey] || !document.defaultView) return;
  const state = document[stateKey] = {visible:true, exits:new WeakMap(), projections:new Set()};
  const stop = () => {
    for (const animation of document.getAnimations()) if(animation.id.startsWith('sd-motion:'))animation.cancel();
    for (const ghost of state.projections) ghost.remove();
    state.projections.clear();
  };
  document.addEventListener('visibilitychange', () => { if (document.hidden) stop(); });
  document.defaultView.matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', event => { if (event.matches) stop(); });
  const visibility = visible => {state.visible=visible;if(!visible)stop();};
  document.defaultView.addEventListener('sodeck:mount',event=>visibility(event.detail.visible!==false));
  document.defaultView.addEventListener('sodeck:visibilityChanged',event=>visibility(event.detail.visible));
  document.defaultView.addEventListener('sodeck:unmount',()=>visibility(false));
  document.addEventListener('toggle', event => {
    if(event.target.tagName!=='DETAILS'||!event.target.open)return;
    for(const child of [...event.target.children].filter(node=>node.tagName!=='SUMMARY').slice(0,4))revealMotion(child);
  },true);
}
export function animateMotion(element, frames, kind = 'enter') {
  const duration = motionDuration(element, kind);
  cancelMotion(element);
  if (!duration || !element.isConnected || !element.getClientRects().length || !element.animate) return;
  observeMotion(element.ownerDocument);
  const animation = element.animate(frames, {duration, easing});
  animation.id = `sd-motion:${kind}`;
  animation.finished.catch(() => {});
  return animation;
}
export function cancelMotion(element) {
  for (const old of element.getAnimations?.() ?? []) if (old.id.startsWith('sd-motion:')) old.cancel();
}
export function enterMotion(element, direction = 'y') {
  observeMotion(element.ownerDocument);
  element.ownerDocument[stateKey]?.exits.get(element)?.remove();
  const current = element.ownerDocument.defaultView.getComputedStyle(element);
  const interrupted = (element.getAnimations?.() ?? []).some(a => a.id.startsWith('sd-motion:'));
  return animateMotion(element, [interrupted ? {opacity:current.opacity, transform:current.transform} : {opacity:0, transform:current.getPropertyValue('--sd-motion-from').trim() || (direction === 'x' ? 'translateX(-8px)' : direction === 'back' ? 'translateX(8px)' : 'translateY(-4px)')}, {opacity:1, transform:'none'}]);
}
export function setMotionText(element, value) {
  const text=String(value??'');
  if(element.textContent===text)return;
  element.textContent=text;
}
export function revealMotion(element) {
  return animateMotion(element,[{opacity:0,clipPath:'inset(0 0 100% 0)'},{opacity:1,clipPath:'inset(0 0 0 0)'}]);
}
function onScreen(node, r=node.getBoundingClientRect()) {
  const view=node.ownerDocument.defaultView;
  return r.width>0&&r.height>0&&r.bottom>0&&r.right>0&&r.top<view.innerHeight&&r.left<view.innerWidth;
}
export function captureMotion(root, selector = '[data-motion="item"]') {
  const positions=new Map();
  if(!motionDuration(root,'layout'))return positions;
  for(const node of root.querySelectorAll(selector)) {
    const rect=node.getBoundingClientRect();
    if(onScreen(node,rect))positions.set(node,rect);
    if(positions.size===40)break;
  }
  return positions;
}
export function settleMotion(root, before, selector = '[data-motion="item"]', enter = true) {
  // Measure the destination without the previous effect, after capturing its presentation.
  for(const node of before.keys())cancelMotion(node);
  for (const [node,next] of captureMotion(root,selector)) {
    const old = before.get(node);
    if (!old) { if(enter)enterMotion(node); continue; }
    const x = old.left - next.left, y = old.top - next.top;
    if (Math.abs(x) > 1 || Math.abs(y) > 1) animateMotion(node, [{transform:`translate(${x}px, ${y}px)`}, {transform:'none'}], 'layout');
  }
}
export function exitMotion(element) {
  if (!motionDuration(element, 'exit') || !element.isConnected || !onScreen(element) || element.querySelector('iframe,canvas,video')) return;
  const nodes = [element, ...element.querySelectorAll('*')];
  // Bound snapshots to small chrome; large views retain their live Surface.
  if (nodes.length > 80) return;
  const document = element.ownerDocument, view = document.defaultView, rect = element.getBoundingClientRect(), state=document[stateKey];
  state.exits.get(element)?.remove();
  const opacity=view.getComputedStyle(element).opacity;
  const ghost = document.createElement('div');
  ghost.innerHTML = element.innerHTML;
  const copies = [ghost, ...ghost.querySelectorAll('*')];
  for (let i = 0; i < nodes.length; i++) {
    const original = nodes[i], copy = copies[i], style = view.getComputedStyle(original);
    for (const attribute of [...copy.attributes]) if (attribute.name === 'id' || attribute.name === 'class' || attribute.name === 'role' || attribute.name === 'tabindex' || attribute.name.startsWith('data-')) copy.removeAttribute(attribute.name);
    copy.style.cssText = [...style].map(name => `${name}:${style.getPropertyValue(name)};`).join('');
    if ('value' in original && 'value' in copy) {copy.value = original.type === 'password' ? '' : original.value;if(original.type==='password')copy.removeAttribute('value');}
  }
  ghost.inert = true; ghost.setAttribute('aria-hidden','true');
  Object.assign(ghost.style, {position:'fixed', left:`${rect.left}px`, top:`${rect.top}px`, width:`${rect.width}px`, height:`${rect.height}px`, margin:'0', transform:'none', animation:'none', transition:'none', pointerEvents:'none', zIndex:'10000'});
  (element.parentElement?.closest('dialog[open]') ?? document.body).append(ghost);
  state.exits.set(element,ghost);state.projections.add(ghost);
  const animation = animateMotion(ghost, [{opacity, transform:'none'}, {opacity:0, transform:view.getComputedStyle(element).getPropertyValue('--sd-motion-from').trim()||'translateY(-4px)'}], 'exit');
  const remove=()=>{ghost.remove();state.projections.delete(ghost);if(state.exits.get(element)===ghost)state.exits.delete(element);};
  if (animation) animation.finished.catch(() => {}).finally(remove); else remove();
}
if (typeof window !== 'undefined') {if(typeof document!=='undefined')observeMotion(document);window.SoDeckMotion = {animateMotion, cancelMotion, enterMotion, exitMotion, setMotionText, revealMotion, captureMotion, settleMotion, observeMotion};}
