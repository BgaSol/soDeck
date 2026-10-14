export interface LogSpan {
  text: string;
  kind?: string;
  color?: string;
  bold?: boolean;
  match?: boolean;
}
export interface HighlightOptions {
  timestamps: boolean;
  highlight: boolean;
  keyword: string;
  caseSensitive: boolean;
}
const colors = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
];
const escaped = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// Only SGR styles become presentation. OSC, cursor moves and other controls disappear.
export function ansiText(raw: string): {
  text: string;
  styles: { from: number; to: number; color?: string; bold?: boolean }[];
} {
  let text = "",
    color: string | undefined,
    bold = false;
  const styles: { from: number; to: number; color?: string; bold?: boolean }[] =
    [];
  const expression =
    /\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][\s\S]*?(?:\x07|\x1b\\|$)|\x1b(?:[^\[]|$)|[\x00-\x08\x0b-\x1f\x7f]/g;
  let position = 0;
  const append = (part: string) => {
    const from = text.length;
    text += part;
    if (color || bold) styles.push({ from, to: text.length, color, bold });
  };
  for (const match of raw.matchAll(expression)) {
    append(raw.slice(position, match.index));
    position = match.index! + match[0].length;
    if (!/^\x1b\[[\d;]*m$/.test(match[0])) continue;
    const codes = (match[0].slice(2, -1) || "0").split(";").map(Number);
    for (let i = 0; i < codes.length; i++) {
      const code = codes[i];
      if (code === 0) {
        color = undefined;
        bold = false;
      } else if (code === 1) bold = true;
      else if (code === 22) bold = false;
      else if (code === 39) color = undefined;
      else if (code >= 30 && code <= 37) color = colors[code - 30];
      else if (code >= 90 && code <= 97) color = colors[code - 90];
      else if (
        code === 38 &&
        codes[i + 1] === 2 &&
        codes.slice(i + 2, i + 5).length === 3
      ) {
        const rgb = codes
          .slice(i + 2, i + 5)
          .map((n) => Math.max(0, Math.min(255, n)));
        color = `rgb(${rgb.join(",")})`;
        i += 4;
      } else if (code === 38 && codes[i + 1] === 5) {
        const n = Math.max(0, Math.min(255, codes[i + 2] ?? 0));
        if (n < 16) color = colors[n % 8];
        else if (n >= 232) {
          const v = 8 + (n - 232) * 10;
          color = `rgb(${v},${v},${v})`;
        } else {
          const v = n - 16,
            scale = [0, 95, 135, 175, 215, 255];
          color = `rgb(${scale[Math.floor(v / 36)]},${scale[Math.floor(v / 6) % 6]},${scale[v % 6]})`;
        }
        i += 2;
      }
    }
  }
  append(raw.slice(position));
  return { text, styles };
}

export function highlightLog(
  raw: string,
  options: HighlightOptions,
): LogSpan[] {
  const parsed = ansiText(raw);
  let text = parsed.text;
  const stamp = /^\d{4}-\d{2}-\d{2}T\S+\s/.exec(text)?.[0] ?? "";
  const shift = options.timestamps ? 0 : stamp.length;
  text = text.slice(shift);
  const ranges: {
    from: number;
    to: number;
    kind?: string;
    color?: string;
    bold?: boolean;
    match?: boolean;
    priority: number;
  }[] = [];
  if (options.highlight) {
    for (const style of parsed.styles) {
      if (style.to > shift)
        ranges.push({
          ...style,
          from: Math.max(0, style.from - shift),
          to: style.to - shift,
          priority: 0,
        });
    }
    if (stamp && options.timestamps)
      ranges.push({
        from: 0,
        to: stamp.length,
        kind: "timestamp",
        priority: 1,
      });
    const json =
      text
        .slice(options.timestamps ? stamp.length : 0)
        .trimStart()
        .startsWith("{") || text.trimStart().startsWith("[");
    if (json) {
      const syntax =
        /"(?:\\.|[^"\\])*"|\b(?:true|false|null)\b|-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/g;
      for (const m of text.matchAll(syntax)) {
        if (m.index! < (options.timestamps ? stamp.length : 0)) continue;
        const kind =
          m[0][0] === '"'
            ? /^\s*:/.test(text.slice(m.index! + m[0].length))
              ? "key"
              : "string"
            : /^(true|false|null)$/.test(m[0])
              ? "literal"
              : "number";
        ranges.push({
          from: m.index!,
          to: m.index! + m[0].length,
          kind,
          priority: 2,
        });
      }
    }
    const exception =
      /\b[\w.$]*(?:Exception|Error)\b|(?:[\w./\\-]+\.(?:java|py|rs|go|js|ts)):\d+(?::\d+)?/g;
    for (const m of text.matchAll(exception))
      ranges.push({
        from: m.index!,
        to: m.index! + m[0].length,
        kind: "exception",
        priority: 3,
      });
  }
  if (options.keyword) {
    const search = new RegExp(
      escaped(options.keyword),
      options.caseSensitive ? "gu" : "giu",
    );
    for (const m of text.matchAll(search))
      ranges.push({
        from: m.index!,
        to: m.index! + m[0].length,
        match: true,
        priority: 4,
      });
  }
  // Sweep boundaries once; avoids quadratic work on long JSON/stack trace records.
  const boundaries = new Map<
    number,
    { start: typeof ranges; end: typeof ranges }
  >();
  const boundary = (at: number) => {
    let b = boundaries.get(at);
    if (!b) {
      b = { start: [], end: [] };
      boundaries.set(at, b);
    }
    return b;
  };
  boundary(0);
  boundary(text.length);
  for (const range of ranges) {
    boundary(range.from).start.push(range);
    boundary(range.to).end.push(range);
  }
  const points = [...boundaries.keys()].sort((a, b) => a - b),
    active = new Set<(typeof ranges)[number]>(),
    spans: LogSpan[] = [];
  for (let i = 0; i < points.length - 1; i++) {
    const at = points[i],
      b = boundaries.get(at)!;
    for (const r of b.end) active.delete(r);
    for (const r of b.start) active.add(r);
    const sorted = [...active].sort((a, b) => a.priority - b.priority);
    const span: LogSpan = { text: text.slice(at, points[i + 1]) };
    for (const r of sorted) {
      if (r.kind) {
        span.kind = r.kind;
        span.color = undefined;
      } else if (r.color) span.color = r.color;
      if (r.bold) span.bold = true;
      if (r.match) span.match = true;
    }
    spans.push(span);
  }
  return spans;
}
