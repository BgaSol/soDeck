// Display effects never delay RPC, focus changes or resource disposal.
const easing = 'cubic-bezier(0.16, 1, 0.3, 1)';
export function motionDuration(element, kind = 'enter') {
  const view = element.ownerDocument.defaultView;
  if (!view || element.ownerDocument.hidden || view.matchMedia('(prefers-reduced-motion: reduce)').matches) return 0;
  const value = parseFloat(view.getComputedStyle(element).getPropertyValue(`--sd-motion-${kind}`));
  return Number.isFinite(value) ? Math.max(0, Math.min(300, value)) : kind === 'layout' ? 200 : kind === 'feedback' || kind === 'exit' ? 100 : 160;
}
export function observeMotion(document) {
  const key=Symbol.for('sodeck.motion.observed');
  if (document[key] || !document.defaultView) return;
  document[key] = true;
  const stop = () => { for (const animation of document.getAnimations()) if(animation.id.startsWith('sd-motion:'))animation.cancel(); };
  document.addEventListener('visibilitychange', () => { if (document.hidden) stop(); });
  document.defaultView.matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', event => { if (event.matches) stop(); });
  document.defaultView.addEventListener('sodeck:visibilityChanged',event=>{if(!event.detail.visible)stop();});
  document.defaultView.addEventListener('sodeck:unmount',stop);
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
  const current = element.ownerDocument.defaultView.getComputedStyle(element);
  const interrupted = (element.getAnimations?.() ?? []).some(a => a.id.startsWith('sd-motion:'));
  return animateMotion(element, [interrupted ? {opacity:current.opacity, transform:current.transform} : {opacity:0, transform:current.getPropertyValue('--sd-motion-from').trim() || (direction === 'x' ? 'translateX(-8px)' : 'translateY(-4px)')}, {opacity:1, transform:'none'}]);
}
export function feedbackMotion(element) {
  const opacity = element.ownerDocument.defaultView.getComputedStyle(element).opacity;
  return animateMotion(element, [{opacity:Math.min(.55, Number(opacity))}, {opacity:1}], 'feedback');
}
export function setMotionText(element, value) {
  const text=String(value??'');
  if(element.textContent===text)return;
  element.textContent=text;feedbackMotion(element);
}
export function revealMotion(element) {
  return animateMotion(element,[{opacity:0,clipPath:'inset(0 0 100% 0)'},{opacity:1,clipPath:'inset(0 0 0 0)'}]);
}
function onScreen(node) {
  const r=node.getBoundingClientRect(),view=node.ownerDocument.defaultView;
  return r.width>0&&r.height>0&&r.bottom>0&&r.right>0&&r.top<view.innerHeight&&r.left<view.innerWidth;
}
export function captureMotion(root, selector = '[data-motion="item"]') {
  return new Map([...root.querySelectorAll(selector)].filter(onScreen).slice(0,40).map(node => [node, node.getBoundingClientRect()]));
}
export function settleMotion(root, before, selector = '[data-motion="item"]') {
  for (const node of [...root.querySelectorAll(selector)].filter(onScreen).slice(0,40)) {
    const old = before.get(node);
    if (!old) { enterMotion(node); continue; }
    const next = node.getBoundingClientRect(), x = old.left - next.left, y = old.top - next.top;
    if (Math.abs(x) > 1 || Math.abs(y) > 1) animateMotion(node, [{transform:`translate(${x}px, ${y}px)`}, {transform:'none'}], 'layout');
  }
}
export function exitMotion(element) {
  if (!motionDuration(element, 'exit') || !element.isConnected || !onScreen(element) || element.querySelector('iframe,canvas,video')) return;
  const nodes = [element, ...element.querySelectorAll('*')];
  // Bound snapshots to small chrome; large views retain their live Surface.
  if (nodes.length > 80) return;
  const document = element.ownerDocument, view = document.defaultView, rect = element.getBoundingClientRect();
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
  const animation = animateMotion(ghost, [{opacity:1, transform:'none'}, {opacity:0, transform:view.getComputedStyle(element).getPropertyValue('--sd-motion-from').trim()||'translateY(-4px)'}], 'exit');
  if (animation) animation.finished.catch(() => {}).finally(() => ghost.remove()); else ghost.remove();
}
if (typeof window !== 'undefined') window.SoDeckMotion = {animateMotion, cancelMotion, enterMotion, exitMotion, feedbackMotion, setMotionText, revealMotion, captureMotion, settleMotion, observeMotion};
