import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { terminalHistoryApi, type HistoryRow } from "./terminalHistoryApi";
import { errorOf } from "./types";
export function TerminalReplayView({
  row,
  onClose,
}: {
  row: HistoryRow;
  onClose: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState("正在恢复历史屏幕…");
  const [playAll, setPlayAll] = useState(false);
  useEffect(() => {
    let cancelled = false;
    const term = new Terminal({
      disableStdin: true,
      cursorBlink: false,
      scrollback: 2000,
      cols: 100,
      rows: 30,
      fontSize: 13,
      theme: { background: "#14171e", foreground: "#dce3ef" },
    });
    term.open(host.current!);
    // No input handler, fit addon, link opener or clipboard bridge is attached to this player.
    void (async () => {
      let after = "0";
      try {
        const through = playAll
          ? (await terminalHistoryApi.status(row.sessionId)).savedSeq
          : row.seq;
        while (!cancelled) {
          const page = await terminalHistoryApi.replay(
            row.sessionId,
            after,
            through,
            playAll ? undefined : row.offset,
          );
          if (cancelled) return;
          for (const event of page.events) {
            if (cancelled) return;
            if (event.kind === "resize") term.resize(event.cols, event.rows);
            else if (event.kind === "output")
              await new Promise<void>((resolve) =>
                term.write(
                  Uint8Array.from(atob(event.data), (c) => c.charCodeAt(0)),
                  resolve,
                ),
              );
            else if (
              event.kind === "clear" &&
              term.buffer.active.type !== "alternate"
            )
              term.clear();
            else if (event.kind === "gap")
              term.writeln(`\r\n[记录缺口：${event.data}]\r\n`);
          }
          setStatus(`只读回放 · 已读取事件 ${page.after}`);
          if (!page.hasMore || page.after === after) break;
          after = page.after;
        }
        if (!cancelled)
          setStatus(
            playAll ? "只读回放 · 已到保存末尾" : "只读回放 · 已定位到历史记录",
          );
      } catch (e) {
        if (!cancelled) setStatus(errorOf(e).message);
      }
    })();
    return () => {
      cancelled = true;
      term.dispose();
    };
  }, [row, playAll]);
  return (
    <div className="terminal-replay" role="dialog" aria-label="历史屏幕回放">
      <header>
        <span role="status">{status}</span>
        <button onClick={() => setPlayAll(!playAll)}>
          {playAll ? "定位命中位置" : "读取到会话末尾"}
        </button>
        <button autoFocus onClick={onClose}>
          关闭回放
        </button>
      </header>
      <div className="terminal-replay-scroll">
        <div ref={host} />
      </div>
    </div>
  );
}
