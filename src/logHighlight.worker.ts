import { highlightLog, type HighlightOptions } from "./logHighlight";
const cache = new Map<
  string,
  { spans: ReturnType<typeof highlightLog>; bytes: number }
>();
let cacheBytes = 0;
self.onmessage = (
  event: MessageEvent<{ id: number; raw: string; options: HighlightOptions }>,
) => {
  const { id, raw, options } = event.data;
  const key = JSON.stringify([raw, options]);
  let item = cache.get(key);
  if (item) {
    cache.delete(key);
    cache.set(key, item);
  } else {
    const spans = highlightLog(raw, options);
    item = {
      spans,
      bytes:
        key.length * 2 +
        spans.reduce((sum, span) => sum + span.text.length * 2 + 64, 0),
    };
    cache.set(key, item);
    cacheBytes += item.bytes;
    while (cacheBytes > 4 * 1024 * 1024 || cache.size > 256) {
      const first = cache.keys().next().value!;
      cacheBytes -= cache.get(first)!.bytes;
      cache.delete(first);
    }
  }
  self.postMessage({ id, spans: item.spans });
};
