import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  type CSSProperties,
} from "react";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";
import type { LogRow, LogLifecycleEvent } from "../logApi";
import { LogText } from "../LogText";
import type { HighlightOptions } from "../logHighlight";
import type { ReadAction, ReadState } from "./model";
export interface Anchor {
  key: string;
  offset: number;
}
export interface ViewerHandle {
  capture: () => Anchor | null;
  restore: (anchor: Anchor | null) => void;
  latest: () => void;
}
type Item = {
  key: string;
  seq: number;
  row?: LogRow;
  event?: LogLifecycleEvent;
};
interface Props {
  rows: LogRow[];
  events: LogLifecycleEvent[];
  options: HighlightOptions;
  wrap: boolean;
  fontSize: number;
  tty: boolean;
  read: ReadState;
  live: boolean;
  busy: boolean;
  older: boolean;
  newer: boolean;
  context: number | null;
  keyword: string;
  selectedFrom: string;
  selectedTo: string;
  onRead: (a: ReadAction) => void;
  onOlder: () => void;
  onNewer: () => void;
  onSelect: (seq: number, extend: boolean) => void;
  onContext: (seq: number) => void;
  empty: string;
}
export const LogViewer = forwardRef<ViewerHandle, Props>(
  function LogViewer(p, ref) {
    const host = useRef<HTMLDivElement>(null),
      scroller = useRef<HTMLElement | null>(null),
      list = useRef<VirtuosoHandle>(null),
      anchor = useRef<Anchor | null>(null),
      intent = useRef(0),
      program = useRef(false),
      previousTop = useRef(0),
      frame = useRef(0),
      data = useRef<Item[]>([]);
    const follow = p.read.mode === "follow" && p.live && p.context === null;
    const latestProps = useRef(p);
    latestProps.current = p;
    const items = useMemo(() => {
      const low = p.rows[0]?.seq ?? 0,
        high = p.rows.at(-1)?.seq ?? 0;
      const value: Item[] = p.rows.map((row) => ({
        key: "log:" + row.seq,
        seq: row.seq,
        row,
      }));
      for (const event of p.events) {
        if (
          !p.rows.length ||
          (event.anchorSeq >= low - 1 && event.anchorSeq <= high)
        )
          value.push({
            key: "event:" + event.id,
            seq: event.anchorSeq + 0.5,
            event,
          });
      }
      value.sort(
        (a, b) => a.seq - b.seq || (a.event?.id ?? 0) - (b.event?.id ?? 0),
      );
      if (
        p.read.mode === "paused" &&
        p.read.reason === "selection" &&
        (!!p.selectedFrom || !!window.getSelection()?.toString()) &&
        data.current.length
      )
        return data.current;
      data.current = value;
      return value;
    }, [p.rows, p.events, p.read, p.selectedFrom]);
    function capture() {
      const scroll = scroller.current;
      if (!scroll) return null;
      const top = scroll.getBoundingClientRect().top;
      for (const element of scroll.querySelectorAll<HTMLElement>(
        "[data-item-key]",
      )) {
        const rect = element.getBoundingClientRect();
        if (rect.bottom > top) {
          return { key: element.dataset.itemKey!, offset: rect.top - top };
        }
      }
      return null;
    }
    function jump(index: number, offset = 0) {
      program.current = true;
      list.current?.scrollToIndex({ index, align: "start", offset });
      cancelAnimationFrame(frame.current);
      frame.current = requestAnimationFrame(() => {
        program.current = false;
        previousTop.current = scroller.current?.scrollTop ?? 0;
      });
    }
    useImperativeHandle(ref, () => ({
      capture,
      restore: (a) => {
        anchor.current = a;
      },
      latest: () => {
        anchor.current = null;
        program.current = true;
        list.current?.scrollToIndex({ index: "LAST", align: "end" });
        requestAnimationFrame(() => {
          program.current = false;
        });
      },
    }));
    useLayoutEffect(() => {
      if (!items.length) return;
      if (follow) {
        program.current = true;
        list.current?.scrollToIndex({ index: "LAST", align: "end" });
        frame.current = requestAnimationFrame(() => {
          program.current = false;
          anchor.current = capture();
        });
      } else if (anchor.current) {
        const a = anchor.current;
        const index = items.findIndex((i) => i.key === a.key);
        if (index >= 0) jump(index, -a.offset);
      }
    }, [items, follow, p.wrap, p.fontSize]);
    useEffect(() => {
      if (p.context !== null) {
        anchor.current = { key: "log:" + p.context, offset: 0 };
        const index = items.findIndex((i) => i.row?.seq === p.context);
        if (index >= 0) jump(index);
      }
    }, [p.context, items]);
    useEffect(() => {
      const selection = () => {
        const s = window.getSelection();
        if (
          s &&
          !s.isCollapsed &&
          s.anchorNode &&
          host.current?.contains(s.anchorNode)
        ) {
          anchor.current = capture();
          latestProps.current.onRead({ type: "pause", reason: "selection" });
        }
      };
      document.addEventListener("selectionchange", selection);
      return () => {
        document.removeEventListener("selectionchange", selection);
        cancelAnimationFrame(frame.current);
      };
    }, []);
    function userIntent() {
      intent.current = Date.now() + 1200;
      program.current = false;
    }
    function scroll() {
      const el = scroller.current;
      if (!el) return;
      const up = el.scrollTop < previousTop.current - 1;
      previousTop.current = el.scrollTop;
      if (!program.current) {
        anchor.current = capture();
        if (up && intent.current > Date.now())
          p.onRead({ type: "pause", reason: "browse" });
        if (el.scrollHeight - el.clientHeight - el.scrollTop <= 4)
          p.onRead({
            type: "bottom",
            userInitiated: intent.current > Date.now(),
            hasSelection: !!window.getSelection()?.toString(),
            live: p.live && p.context === null,
          });
      }
    }
    return (
      <div
        ref={host}
        className={"log-viewer" + (p.wrap ? " log-wrap" : "")}
        style={{ "--log-font-size": p.fontSize / 14 + "rem" } as CSSProperties}
        onWheel={(e) => {
          userIntent();
          if (e.deltaY < 0) {
            anchor.current = capture();
            p.onRead({ type: "pause", reason: "browse" });
          }
        }}
        onPointerDown={userIntent}
        onKeyDown={(e) => {
          if (
            [
              "ArrowUp",
              "PageUp",
              "Home",
              "ArrowDown",
              "PageDown",
              "End",
              " ",
            ].includes(e.key)
          ) {
            userIntent();
            if (["ArrowUp", "PageUp", "Home"].includes(e.key)) {
              anchor.current = capture();
              p.onRead({ type: "pause", reason: "browse" });
            }
          }
        }}
      >
        <Virtuoso
          ref={list}
          className="log-virtual-list"
          data={items}
          scrollerRef={(el) => {
            if (el instanceof HTMLElement) {
              if (scroller.current && scroller.current !== el)
                scroller.current.onscroll = null;
              scroller.current = el;
              el.tabIndex = 0;
              el.setAttribute("aria-label", "容器日志内容");
              el.onscroll = scroll;
            }
          }}
          computeItemKey={(_, item) => item.key}
          defaultItemHeight={26}
          increaseViewportBy={{ top: 200, bottom: 200 }}
          alignToBottom
          followOutput={follow ? "auto" : false}
          atBottomStateChange={(atBottom) => {
            if (atBottom)
              p.onRead({
                type: "bottom",
                userInitiated: intent.current > Date.now() && !program.current,
                hasSelection: !!window.getSelection()?.toString(),
                live: p.live && p.context === null,
              });
          }}
          components={{
            Header: () =>
              p.older ? (
                <div className="log-page-control">
                  <button
                    disabled={p.busy}
                    onClick={() => {
                      anchor.current = capture();
                      p.onRead({ type: "pause", reason: "browse" });
                      p.onOlder();
                    }}
                  >
                    加载更早日志
                  </button>
                </div>
              ) : null,
            Footer: () =>
              p.newer && !follow ? (
                <div className="log-page-control">
                  <button disabled={p.busy} onClick={p.onNewer}>
                    加载后续日志
                  </button>
                </div>
              ) : null,
          }}
          itemContent={(_, item) =>
            item.event ? (
              <div className="log-system-event" data-item-key={item.key}>
                <span>
                  {new Date(item.event.observedAt).toLocaleTimeString()}
                </span>
                <span>{item.event.detail}</span>
                <span className="log-event-kind">运行事件</span>
              </div>
            ) : (
              <div
                data-item-key={item.key}
                data-seq={item.row!.seq}
                className={
                  "log-row" +
                  (p.options.highlight && item.row!.level
                    ? " log-level-" + item.row!.level
                    : "") +
                  (Number(p.selectedFrom) <= item.row!.seq &&
                  Number(p.selectedTo) >= item.row!.seq
                    ? " log-row-selected"
                    : "")
                }
              >
                <button
                  className="log-seq"
                  title="点击选择记录，Shift 点击扩展范围"
                  onClick={(e) => p.onSelect(item.row!.seq, e.shiftKey)}
                >
                  #{item.row!.seq}
                </button>
                <div className="log-row-body">
                  {item.row!.stream === "stderr" && !p.tty && (
                    <span className="log-stream">[stderr] </span>
                  )}
                  {item.row!.continuation && (
                    <span className="log-fragment">续行 </span>
                  )}
                  <LogText
                    raw={(p.keyword && item.row!.prefix
                      ? item.row!.prefix + item.row!.text
                      : item.row!.raw
                    ).slice(0, 12000)}
                    options={p.options}
                  />
                  {item.row!.raw.length > 12000 && (
                    <span className="log-fragment">
                      {" "}
                      …长行已折叠，选择记录可复制全文
                    </span>
                  )}
                  {!item.row!.complete && !item.row!.continuation && (
                    <span className="log-fragment"> 未换行</span>
                  )}
                </div>
                {!!p.keyword && p.context === null && (
                  <button
                    className="log-context"
                    onClick={() => p.onContext(item.row!.seq)}
                  >
                    上下文
                  </button>
                )}
              </div>
            )
          }
        />
        {!items.length && !p.busy && <p className="log-empty">{p.empty}</p>}
      </div>
    );
  },
);
