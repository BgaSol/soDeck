import { memo, useEffect, useState } from "react";
import { ansiText, type HighlightOptions, type LogSpan } from "./logHighlight";
let worker: Worker | undefined,
  nextId = 0;
const pending = new Map<number, (spans: LogSpan[]) => void>();
function tokenWorker() {
  if (!worker) {
    worker = new Worker(new URL("./logHighlight.worker.ts", import.meta.url), {
      type: "module",
    });
    worker.onmessage = (
      event: MessageEvent<{ id: number; spans: LogSpan[] }>,
    ) => {
      const callback = pending.get(event.data.id);
      pending.delete(event.data.id);
      callback?.(event.data.spans);
    };
  }
  return worker;
}
export const LogText = memo(function LogText({
  raw,
  options,
}: {
  raw: string;
  options: HighlightOptions;
}) {
  const [spans, setSpans] = useState<LogSpan[] | null>(null);
  useEffect(() => {
    setSpans(null);
    const id = ++nextId;
    pending.set(id, setSpans);
    tokenWorker().postMessage({ id, raw, options });
    return () => {
      pending.delete(id);
    };
  }, [raw, options]);
  return (
    <>
      {spans
        ? spans.map((span, index) => (
            <span
              key={index}
              className={`${span.kind ? `log-token-${span.kind}` : ""}${span.match ? " log-search-match" : ""}${span.color && !span.color.startsWith("rgb") ? ` log-ansi-${span.color}` : ""}`}
              style={{
                color: span.color?.startsWith("rgb") ? span.color : undefined,
                fontWeight: span.bold ? 600 : undefined,
              }}
            >
              {span.text}
            </span>
          ))
        : ansiText(raw).text.replace(
            options.timestamps ? /$^/ : /^\d{4}-\d{2}-\d{2}T\S+\s/,
            "",
          )}
    </>
  );
});
