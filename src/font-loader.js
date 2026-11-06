// Used by core previews and plugin-owned terminal/editor UIs. Assets stay in plugin packages.
const cache = new Map();
let sequence = 0;
const quote = text => JSON.stringify(String(text));
export async function loadFont(value, presets, resolve) {
  const preset = value?.source === 'preset' ? presets.find(p => p.id === value.id) : undefined;
  if (value?.source !== 'local' && !preset) throw new Error('预置字体不存在');
  const sources = value.source === 'local'
    ? [`local(${quote(value.family)})`, `local(${quote(value.family)})`]
    : [preset.regular, preset.bold].map(path => `url(${quote(resolve(path))}) format("woff2")`);
  const key = JSON.stringify(sources);
  if (cache.has(key)) return cache.get(key);
  const promise = (async () => {
    const family = 'SoDeckFont' + ++sequence;
    const faces = [400, 700].map((weight, i) => new FontFace(family, sources[i], { weight: String(weight), style: 'normal' }));
    let timer;
    try {
      await Promise.race([Promise.all(faces.map(face => face.load())), new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('字体加载超过 2.5 秒')), 2500); })]);
      faces.forEach(face => document.fonts.add(face));
      return `"${family}", monospace`;
    } catch (error) { cache.delete(key); throw new Error(value.source === 'local' ? `找不到本机字体“${value.family}”` : `无法加载 ${preset.title}：${error.message}`); }
    finally { clearTimeout(timer); }
  })();
  cache.set(key, promise);
  return promise;
}
