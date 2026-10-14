export type LogStream = "stdout" | "stderr";
interface LogLine {
  id: number;
  stream: LogStream;
  text: string;
  bytes: number;
  complete: boolean;
}
const encoder = new TextEncoder();
// Only printable text reaches the viewer. Escape sequences may span SSH packets.
class PlainTextDecoder {
  private decoder = new TextDecoder();
  private escape: "text" | "esc" | "csi" | "osc" | "oscEnd" = "text";
  decode(bytes?: Uint8Array) {
    const text = this.decoder.decode(bytes, { stream: !!bytes });
    let result = "";
    for (const c of text) {
      if (this.escape === "oscEnd") {
        this.escape = c === "\\" ? "text" : "osc";
        continue;
      }
      if (this.escape === "osc") {
        if (c === "\x07") this.escape = "text";
        else if (c === "\x1b") this.escape = "oscEnd";
        continue;
      }
      if (this.escape === "csi") {
        if (c >= "@" && c <= "~") this.escape = "text";
        continue;
      }
      if (this.escape === "esc") {
        this.escape = c === "[" ? "csi" : c === "]" ? "osc" : "text";
        continue;
      }
      if (c === "\x1b") {
        this.escape = "esc";
        continue;
      }
      if (c === "\n" || c === "\t" || (c >= " " && c !== "\x7f")) result += c;
    }
    return result;
  }
}

export class LogBuffer {
  lines: LogLine[] = [];
  bytes = 0;
  dropped = 0;
  private nextId = 1;
  private pending = new Map<LogStream, LogLine>();
  private decoders = {
    stdout: new PlainTextDecoder(),
    stderr: new PlainTextDecoder(),
  };
  constructor(
    readonly maxLines = 10_000,
    readonly maxBytes = 10 * 1024 * 1024,
  ) {}
  append(stream: LogStream, bytes?: Uint8Array) {
    const parts = this.decoders[stream].decode(bytes).split("\n");
    parts.forEach((part, index) => {
      if (!part && index === parts.length - 1) return;
      let row = this.pending.get(stream);
      if (!row) {
        row = {
          id: this.nextId++,
          stream,
          text: "",
          bytes: 0,
          complete: false,
        };
        this.pending.set(stream, row);
        this.lines.push(row);
      }
      row.text += part;
      const count = encoder.encode(part).length;
      row.bytes += count;
      this.bytes += count;
      if (index < parts.length - 1) {
        row.complete = true;
        this.pending.delete(stream);
      }
      // A single unbroken line cannot bypass the memory cap.
      if (row.bytes > this.maxBytes) {
        let tail = row.text.slice(-Math.floor(this.maxBytes / 4));
        if (/^[\uDC00-\uDFFF]/.test(tail)) tail = tail.slice(1);
        this.bytes -= row.bytes;
        row.text = tail;
        row.bytes = encoder.encode(tail).length;
        this.bytes += row.bytes;
        this.dropped++;
      }
      while (this.lines.length > this.maxLines || this.bytes > this.maxBytes) {
        const removed = this.lines.shift()!;
        this.bytes -= removed.bytes;
        this.dropped++;
        if (this.pending.get(removed.stream) === removed)
          this.pending.delete(removed.stream);
      }
    });
  }
  render(timestamps: boolean, tty = false) {
    return this.lines
      .map((row) => {
        const value = timestamps
          ? row.text
          : row.text.replace(/^\d{4}-\d{2}-\d{2}T\S+\s/, "");
        return (row.stream === "stderr" && !tty ? "[stderr] " : "") + value;
      })
      .join("\n");
  }
}

// Return a single minimal contiguous replacement, preserving CodeMirror selection
// and scroll state outside the updated range without resetting the EditorState.
export function logChange(previous: string, next: string) {
  let start = 0;
  while (
    start < previous.length &&
    start < next.length &&
    previous[start] === next[start]
  )
    start++;
  let end = previous.length,
    newEnd = next.length;
  while (
    end > start &&
    newEnd > start &&
    previous[end - 1] === next[newEnd - 1]
  ) {
    end--;
    newEnd--;
  }
  return { from: start, to: end, insert: next.slice(start, newEnd) };
}
