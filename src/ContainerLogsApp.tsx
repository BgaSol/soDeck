import { useCallback, useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { search, searchKeymap, openSearchPanel } from "@codemirror/search";
import { ScrollText, RefreshCw, Search, ArrowDownToLine } from "lucide-react";
import { errorOf } from "./types";
import { LogBuffer, logChange, type LogStream } from "./logBuffer";
import "./files.css";
import "./logs.css";

type Info = {
  serverName: string;
  containerName: string;
  containerId: string;
  endpoint: string;
};
type LogEvent =
  | { type: "ready"; following: boolean; tty: boolean }
  | { type: "data"; stream: LogStream; data: string; bytes: number }
  | { type: "end"; error: string | null };

export function ContainerLogsApp() {
  const [info, setInfo] = useState<Info | null>(null);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("正在连接…");
  const [tail, setTail] = useState(1000);
  const [timestamps, setTimestamps] = useState(true);
  const [wrap, setWrap] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const [stats, setStats] = useState({ lines: 0, dropped: 0, tty: false });
  const parent = useRef<HTMLDivElement>(null);
  const editor = useRef<EditorView | null>(null);
  const wrapping = useRef(new Compartment());
  const buffer = useRef(new LogBuffer());
  const text = useRef("");
  const epoch = useRef(0);
  const options = useRef({ timestamps: true, autoScroll: true, tty: false });
  const dirty = useRef(false);
  const pauseAutoScroll = useCallback(() => {
    options.current.autoScroll = false;
    setAutoScroll(false);
  }, []);
  const render = useCallback(() => {
    const view = editor.current;
    if (!view || !dirty.current) return;
    dirty.current = false;
    const next = buffer.current.render(
      options.current.timestamps,
      options.current.tty,
    );
    const scroll = view.scrollDOM;
    const top = scroll.scrollTop,
      left = scroll.scrollLeft;
    const dropped = buffer.current.dropped - statsRef.current.dropped;
    const lineHeight = view.defaultLineHeight;
    if (next !== text.current) {
      view.dispatch({
        changes: logChange(text.current, next),
        ...(options.current.autoScroll
          ? { effects: EditorView.scrollIntoView(next.length, { y: "end" }) }
          : {}),
      });
      text.current = next;
      // scrollIntoView also moves horizontally; following logs should only
      // move vertically and preserve the user's horizontal reading position.
      if (options.current.autoScroll) {
        requestAnimationFrame(() => {
          if (editor.current === view) scroll.scrollLeft = left;
        });
      }
      if (!options.current.autoScroll) {
        view.requestMeasure({
          read: () => null,
          write: () => {
            scroll.scrollTop = Math.max(
              0,
              top - Math.max(0, dropped) * lineHeight,
            );
            scroll.scrollLeft = left;
          },
        });
      }
    }
    const current = {
      lines: buffer.current.lines.length,
      dropped: buffer.current.dropped,
      tty: options.current.tty,
    };
    statsRef.current = current;
    setStats(current);
  }, []);
  const statsRef = useRef({ lines: 0, dropped: 0, tty: false });
  const start = useCallback(
    async (lines: number) => {
      const token = ++epoch.current;
      setError("");
      setStatus("正在连接…");
      buffer.current = new LogBuffer();
      dirty.current = true;
      options.current.tty = false;
      render();
      let runId = "",
        unacked = 0;
      const ack = () => {
        if (runId && unacked) {
          const bytes = unacked;
          unacked = 0;
          void invoke("logs_ack", { runId, bytes }).catch(() => {});
        }
      };
      const output = new Channel<LogEvent>();
      output.onmessage = (event) => {
        if (epoch.current !== token) return;
        if (event.type === "ready") {
          options.current.tty = event.tty;
          dirty.current = true;
          setStatus(event.following ? "实时接收中" : "正在读取历史日志…");
        } else if (event.type === "data") {
          buffer.current.append(
            event.stream,
            Uint8Array.from(atob(event.data), (c) => c.charCodeAt(0)),
          );
          dirty.current = true;
          unacked += event.bytes;
          ack();
        } else {
          buffer.current.append("stdout");
          buffer.current.append("stderr");
          dirty.current = true;
          render();
          setStatus(event.error ? "读取已中断" : "日志读取已结束");
          setError(event.error || "");
        }
      };
      try {
        runId = await invoke<string>("logs_start", { tail: lines, output });
        ack();
      } catch (e) {
        if (epoch.current === token) {
          setError(errorOf(e).message);
          setStatus("读取失败");
        }
      }
    },
    [render],
  );
  useEffect(() => {
    const view = new EditorView({
      parent: parent.current!,
      state: EditorState.create({
        extensions: [
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          EditorView.contentAttributes.of({
            "aria-label": "容器日志内容",
            tabindex: "0",
          }),
          search({ top: true }),
          keymap.of([
            {
              key: "Mod-f",
              run: (view) => {
                pauseAutoScroll();
                return openSearchPanel(view);
              },
            },
            ...searchKeymap,
            ...defaultKeymap,
          ]),
          wrapping.current.of([]),
          EditorView.theme({
            "&": {
              height: "100%",
              backgroundColor: "var(--surface)",
              color: "var(--text)",
            },
            ".cm-scroller": {
              fontFamily: "var(--mono)",
              fontSize: "12px",
              lineHeight: "1.6",
              overflow: "auto",
            },
            ".cm-content": { padding: "10px 14px" },
            "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
              backgroundColor: "var(--selected)",
            },
            ".cm-panels": {
              backgroundColor: "var(--canvas)",
              color: "var(--text)",
            },
            ".cm-searchMatch": {
              backgroundColor: "#d6b64055",
              outline: "1px solid #b8972e",
            },
          }),
        ],
      }),
    });
    editor.current = view;
    let previousTop = 0,
      dragging = false;
    const wheel = (event: WheelEvent) => {
      if (event.deltaY < 0) pauseAutoScroll();
    };
    const key = (event: KeyboardEvent) => {
      if (["ArrowUp", "PageUp", "Home"].includes(event.key)) pauseAutoScroll();
    };
    const pointerDown = () => {
      dragging = true;
    };
    const pointerUp = () => {
      dragging = false;
    };
    const scroll = () => {
      const top = view.scrollDOM.scrollTop;
      if (
        dragging &&
        options.current.autoScroll &&
        top < previousTop - 1 &&
        view.scrollDOM.scrollHeight - top - view.scrollDOM.clientHeight > 5
      )
        pauseAutoScroll();
      previousTop = top;
    };
    view.scrollDOM.addEventListener("scroll", scroll);
    view.scrollDOM.addEventListener("wheel", wheel, { passive: true });
    view.scrollDOM.addEventListener("keydown", key);
    view.scrollDOM.addEventListener("pointerdown", pointerDown);
    window.addEventListener("pointerup", pointerUp);
    const timer = window.setInterval(render, 100);
    let disposed = false;
    void invoke<Info>("logs_info")
      .then((value) => {
        if (!disposed) {
          setInfo(value);
          void start(1000);
        }
      })
      .catch((e) => {
        if (!disposed) {
          setError(errorOf(e).message);
          setStatus("读取失败");
        }
      });
    return () => {
      disposed = true;
      epoch.current++;
      clearInterval(timer);
      window.removeEventListener("pointerup", pointerUp);
      view.destroy();
      editor.current = null;
      void invoke("logs_stop").catch(() => {});
    };
  }, [render, start, pauseAutoScroll]);
  useEffect(() => {
    options.current.timestamps = timestamps;
    dirty.current = true;
    render();
  }, [timestamps, render]);
  useEffect(() => {
    editor.current?.dispatch({
      effects: wrapping.current.reconfigure(
        wrap ? EditorView.lineWrapping : [],
      ),
    });
  }, [wrap]);
  function scrollLatest() {
    options.current.autoScroll = true;
    setAutoScroll(true);
    const view = editor.current;
    if (view) {
      const left = view.scrollDOM.scrollLeft;
      view.dispatch({
        effects: EditorView.scrollIntoView(view.state.doc.length, { y: "end" }),
      });
      requestAnimationFrame(() => {
        if (editor.current === view) view.scrollDOM.scrollLeft = left;
      });
    }
  }
  return (
    <div className="container-logs-app remote-editor-app">
      <header className="remote-editor-header">
        <ScrollText size={20} />
        <div className="log-title">
          <h1>
            {info
              ? `${info.serverName} / ${info.containerName} / 日志`
              : "容器日志"}
          </h1>
          <p title={info?.containerId}>
            {info
              ? `${info.endpoint} · ${info.containerId.slice(0, 12)}`
              : "正在确认容器…"}
          </p>
        </div>
        <span className="log-status" role="status">
          {status}
        </span>
      </header>
      <div className="log-tools">
        <label>
          最近{" "}
          <select
            aria-label="历史日志行数"
            value={tail}
            onChange={(e) => setTail(Number(e.target.value))}
          >
            {[100, 500, 1000, 5000].map((n) => (
              <option key={n} value={n}>
                {n} 行
              </option>
            ))}
          </select>
        </label>
        <button
          disabled={!info || status === "正在连接…"}
          onClick={() => void start(tail)}
        >
          <RefreshCw size={14} /> {error ? "重新连接" : "重新加载"}
        </button>
        <button
          onClick={() => {
            pauseAutoScroll();
            if (editor.current) openSearchPanel(editor.current);
          }}
        >
          <Search size={14} />
          查找
        </button>
        <label>
          <input
            type="checkbox"
            checked={timestamps}
            onChange={(e) => setTimestamps(e.target.checked)}
          />
          时间戳
        </label>
        <label>
          <input
            type="checkbox"
            checked={wrap}
            onChange={(e) => setWrap(e.target.checked)}
          />
          自动换行
        </label>
      </div>
      {error && (
        <div className="editor-error" role="alert">
          {error}
        </div>
      )}
      <div className="log-viewer">
        <div ref={parent} className="remote-code-editor log-content" />
        {!stats.lines && status !== "正在连接…" && (
          <p className="log-empty">
            {status === "实时接收中"
              ? "暂无日志，正在等待新的输出…"
              : status === "日志读取已结束"
                ? "容器暂无日志"
                : ""}
          </p>
        )}
      </div>
      <footer className="log-footer">
        <span>
          已保留 {stats.lines} 行
          {stats.dropped > 0
            ? ` · 已移除最旧内容（${stats.dropped} 行或片段）`
            : ""}
          {stats.tty ? " · 容器 TTY 输出已合并" : " · [stderr] 标记错误输出流"}
        </span>
        <label>
          <input
            type="checkbox"
            checked={autoScroll}
            onChange={(e) => {
              options.current.autoScroll = e.target.checked;
              setAutoScroll(e.target.checked);
              if (e.target.checked) scrollLatest();
            }}
          />
          自动滚动
        </label>
        <button onClick={scrollLatest}>
          <ArrowDownToLine size={14} />
          跳到最新
        </button>
      </footer>
    </div>
  );
}
