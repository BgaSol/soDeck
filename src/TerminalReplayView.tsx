import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { ArrowLeft, LockKeyhole } from "lucide-react";
import {
  terminalHistoryApi,
  type HistoryRow,
  type HistorySession,
} from "./terminalHistoryApi";
import { errorOf } from "./types";
export function TerminalReplayView({
  row,
  session,
  onClose,
}: {
  row: HistoryRow;
  session?: HistorySession;
  onClose: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState("正在恢复历史屏幕…");
  const [playAll, setPlayAll] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setStatus("正在恢复历史屏幕…");
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
          setStatus("正在恢复历史屏幕…");
          if (!page.hasMore || page.after === after) break;
          after = page.after;
        }
        if (!cancelled)
          setStatus(
            playAll ? "已显示保存末尾的屏幕" : "已恢复选中记录时的屏幕",
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
    <section
      className="terminal-replay"
      aria-label="历史屏幕"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <header>
        <button autoFocus className="history-replay-back" onClick={onClose}>
          <ArrowLeft size={15} aria-hidden="true" />
          返回记录
        </button>
        <div>
          <span className="history-replay-source">
            {session
              ? `${session.source.serverName}${session.source.containerName ? ` / ${session.source.containerName}` : ""}`
              : "历史屏幕"}
          </span>
          <LockKeyhole size={14} aria-hidden="true" />
          只读
        </div>
      </header>
      <div className="terminal-replay-scroll">
        <div ref={host} />
      </div>
      <footer className="history-replay-footer">
        <div>
          {playAll ? (
            <span>会话已保存末尾</span>
          ) : (
            <time dateTime={new Date(row.time).toISOString()}>
              {new Date(row.time).toLocaleString()}
            </time>
          )}
          <p role="status">{status}</p>
        </div>
        <button onClick={() => setPlayAll(!playAll)}>
          {playAll ? "回到选中位置" : "查看保存末尾"}
        </button>
      </footer>
    </section>
  );
}
