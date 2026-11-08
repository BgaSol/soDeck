export function path(points, maximum = 100, gapMs = 30000, end = points.at(-1)?.at || Date.now()) {
  const start = end - 300000;
  let result = '', open = false, previous;
  for (const p of points) {
    if (p.at < start || p.at > end) continue;
    if (p.value == null || !Number.isFinite(p.value)) { open = false; previous = p.at; continue; }
    if (previous != null && p.at - previous > gapMs) open = false;
    const x = (p.at - start) / 300000 * 240;
    const y = 46 - Math.max(0, Math.min(maximum, p.value)) / maximum * 44;
    result += (open ? 'L' : 'M') + x.toFixed(1) + ' ' + y.toFixed(1) + ' ';
    open = true; previous = p.at;
  }
  return result.trim();
}
const node = (name, values) => {
  const element = document.createElementNS('http://www.w3.org/2000/svg', name);
  for (const [key, value] of Object.entries(values)) element.setAttribute(key, value);
  return element;
};
export function draw(svg, series, maximum = 100, gapMs = 30000, format = value => value.toFixed(1) + '%', end = Date.now()) {
  const tooltip = svg.parentElement.querySelector('.chart-tooltip');
  const primary = series[0]?.points || [];
  const points = primary.filter(p => p.at >= end - 300000 && p.at <= end);
  const axis = svg.parentElement.querySelector('.chart-axis');
  if (axis) axis.hidden = !points.some(p => Number.isFinite(p.value));
  svg.replaceChildren();
  for (const y of [2, 24, 46]) svg.append(node('path', { d: 'M0 ' + y + 'H240', class: 'baseline' }));
  for (const s of series) {
    const d = path(s.points, maximum, gapMs, end);
    svg.append(node('path', { d, class: 'series ' + s.className }));
  }
  const title = node('title', {});
  title.textContent = points.length ? '最近五分钟，' + points.length + ' 个采样点。方向键查看历史。' : '等待有效采样。未知值和采样缺口不补零。';
  svg.prepend(title);
  const cursor = node('line', { y1: 1, y2: 47, class: 'cursor-line', visibility: 'hidden' });
  svg.append(cursor);
  let index = points.findIndex(p => String(p.at) === svg.dataset.inspectAt);
  if (index < 0) index = points.length - 1;
  function hide() {
    tooltip.hidden = true; cursor.setAttribute('visibility', 'hidden'); svg.querySelectorAll('.sample-point').forEach(p => p.remove());
  }
  function show(i) {
    if (!points.length) { hide(); return; }
    index = Math.max(0, Math.min(points.length - 1, i));
    const p = points[index], x = (p.at - (end - 300000)) / 300000 * 240;
    svg.dataset.inspectAt = String(p.at);
    cursor.setAttribute('x1', x); cursor.setAttribute('x2', x); cursor.setAttribute('visibility', 'visible');
    svg.querySelectorAll('.sample-point').forEach(p => p.remove());
    const values = series.map(s => {
      const value = s.points.find(sample => sample.at === p.at)?.value;
      if (Number.isFinite(value)) svg.append(node('circle', { cx: x, cy: 46 - Math.max(0, Math.min(maximum, value)) / maximum * 44, r: 2.5, class: 'sample-point ' + s.className }));
      return s.label + ' ' + (Number.isFinite(value) ? format(value) : '采样缺口');
    });
    tooltip.textContent = new Date(p.at).toLocaleTimeString() + ' · ' + values.join(' / ');
    tooltip.hidden = false;
  }
  svg.onpointermove = e => {
    if (!points.length) return;
    const box = svg.getBoundingClientRect(), at = end - 300000 + (e.clientX - box.left) / box.width * 300000;
    let nearest = 0;
    points.forEach((p, i) => { if (Math.abs(p.at - at) < Math.abs(points[nearest].at - at)) nearest = i; });
    show(nearest);
  };
  svg.onpointerleave = () => { if (document.activeElement !== svg) hide(); };
  svg.onfocus = () => show(index);
  svg.onblur = hide;
  svg.onkeydown = e => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End', 'Escape'].includes(e.key)) return;
    e.preventDefault();
    if (e.key === 'Escape') hide();
    else show(e.key === 'Home' ? 0 : e.key === 'End' ? points.length - 1 : index + (e.key === 'ArrowLeft' ? -1 : 1));
  };
  if (document.activeElement === svg && !tooltip.hidden) show(index); else hide();
}
