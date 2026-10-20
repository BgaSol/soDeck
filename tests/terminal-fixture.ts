import type { Page } from "@playwright/test";
export async function setup(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(() => {
    const w = window as any;
    const source = {
      id: "source-container",
      serverId: "server-a",
      serverName: "生产服务器",
      address: "10.0.0.1:22",
      username: "ops",
      containerName: "api-service",
      containerId: "abc".repeat(21),
      endpoint: "unix:///var/run/docker.sock",
    };
    const sessions = [
      {
        id: "container-1",
        workspaceId: "container-window",
        source,
        startedAt: 1792044000000,
        endedAt: null,
        state: "recording",
        savedSeq: "12000",
        indexedSeq: "12000",
        lastRecord: "12000",
        bytes: 8000000,
        error: null,
      },
      {
        id: "server-1",
        workspaceId: "server-window",
        source: {
          ...source,
          id: "source-server",
          serverName: "预发布服务器",
          containerName: null,
          containerId: null,
        },
        startedAt: 1792040000000,
        endedAt: 1792041000000,
        state: "closed",
        savedSeq: "100",
        indexedSeq: "100",
        lastRecord: "100",
        bytes: 8000,
        error: null,
      },
    ];
    const row = (sessionId: string, n: number) => ({
      id: String(n),
      sessionId,
      seq: String(n),
      offset: 2,
      time: 1792044000000 + n * 1000,
      segment: n > 6000 ? 1 : 0,
      kind: n === 6000 ? "marker" : "text",
      text:
        n === 6000
          ? "屏幕已清除 · 以上为清屏前记录"
          : `历史输出 ${n} 中文 ERROR`,
      ansi: "",
    });
    const callbacks = new Map<number, (event: any) => void>();
    const eventListeners = new Map<
      number,
      { event: string; handler: number }
    >();
    let serial = 0;
    let channel: any;
    const state = {
      calls: [] as any[],
      sessions,
      emit: (e: any) => channel?.onmessage(e),
      broadcast: (payload: any) => {
        for (const [id, listener] of eventListeners)
          if (listener.event === "system-settings-changed")
            callbacks.get(listener.handler)?.({
              event: listener.event,
              id,
              payload,
            });
      },
      fail: false,
      clipboard: "echo hello",
      writes: [] as string[],
      failRead: false,
      failWrite: false,
      delayRead: false,
      releaseRead: null as null | (() => void),
      settings: {
        developerMode: false,
        activeDeveloperMode: false,
        revision: 0,
        loadError: null,
        platform: "windows",
        terminalClipboard: {
          shortcutMode: "platform",
          copyOnSelect: false,
          rightClickPaste: false,
        },
      },
    };
    w.__HISTORY_TEST__ = state;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "container-test" },
        currentWebview: { label: "container-test" },
      },
      transformCallback: (fn: (event: any) => void) => {
        const id = ++serial;
        callbacks.set(id, fn);
        return id;
      },
      unregisterCallback: () => {},
      invoke: async (command: string, args: any = {}) => {
        state.calls.push({ command, args });
        if (command === "get_system_settings")
          return structuredClone(state.settings);
        if (command === "set_terminal_clipboard_settings") {
          if (args.expectedRevision !== state.settings.revision)
            throw new Error("设置已在其他窗口更新");
          state.settings = {
            ...state.settings,
            revision: state.settings.revision + 1,
            terminalClipboard: args.preferences,
          };
          return structuredClone(state.settings);
        }
        if (command === "plugin:clipboard-manager|read_text") {
          const text = state.clipboard;
          if (state.delayRead)
            await new Promise<void>((resolve) => {
              state.releaseRead = resolve;
            });
          if (state.failRead) throw new Error("clipboard busy");
          return text;
        }
        if (command === "plugin:clipboard-manager|write_text") {
          if (state.failWrite) throw new Error("clipboard busy");
          state.clipboard = args.text;
          state.writes.push(args.text);
          w.__HISTORY_COPIED__ = args.text;
          return null;
        }
        if (command === "container_info")
          return {
            serverName: "生产服务器",
            containerName: "api-service",
            host: "10.0.0.1",
            port: 22,
            username: "ops",
          };
        if (command === "connect_container") {
          channel = args.output;
          setTimeout(() => {
            channel.onmessage({
              type: "ready",
              connection: {
                target: { name: "生产服务器" },
                hops: [],
                identity: "identity",
              },
            });
            channel.onmessage({
              type: "history",
              sessionId: "container-1",
              sourceId: source.id,
              workspaceId: "container-window",
            });
            channel.onmessage({ type: "data", data: btoa("ops@api:~$ ") });
          }, 10);
          return "container-1";
        }
        if (command === "terminal_clear_display") {
          channel.onmessage({ type: "displayCleared" });
          return null;
        }
        if (command === "terminal_history_status")
          return sessions.find((s) => s.id === args.sessionId);
        if (command === "terminal_history_sessions")
          return structuredClone(sessions);
        if (command === "terminal_history_page") {
          if (state.fail) throw { message: "历史文件暂时无法读取" };
          const q = args.query,
            session = sessions.find((s) => s.id === q.sessionId)!;
          const max = Math.min(
            Number(session.lastRecord),
            Number(q.snapshot ?? session.lastRecord),
          );
          let ids = Array.from({ length: max }, (_, i) => i + 1).filter(
            (id) =>
              id < Number(q.before ?? Infinity) && id > Number(q.after ?? 0),
          );
          const hasMore = ids.length > q.limit;
          ids =
            q.after !== undefined ? ids.slice(0, q.limit) : ids.slice(-q.limit);
          return {
            rows: ids.map((id) => row(session.id, id)),
            hasMore,
            snapshot: String(max),
          };
        }
        if (command === "terminal_history_search") {
          const q = args.query;
          const chosen = sessions.filter(
            (s) =>
              (!q.sessionId || s.id === q.sessionId) &&
              (!q.sourceId || s.source.id === q.sourceId),
          );
          return {
            hits: chosen.map((session) => ({
              session,
              row: row(session.id, session.id === "container-1" ? 15 : 25),
            })),
            cursor: null,
            snapshot: JSON.stringify({
              sessions: chosen.map((s) => [s.id, s.lastRecord]),
            }),
          };
        }
        if (command === "terminal_history_copy")
          return `copied ${args.from}-${args.to}`;
        if (command === "terminal_history_export")
          return "C:\\exports\\history.txt";
        if (command === "terminal_history_replay")
          return {
            events: [
              { seq: "1", time: 0, kind: "resize", cols: 100, rows: 30 },
              {
                seq: "2",
                time: 1,
                kind: "output",
                data: btoa("historical screen"),
              },
            ],
            after: args.through ?? "2",
            hasMore: false,
          };
        if (command === "list_servers" || command === "list_transfers")
          return [];
        if (command === "plugin:event|listen") {
          const id = ++serial;
          eventListeners.set(id, { event: args.event, handler: args.handler });
          return id;
        }
        if (command === "plugin:event|unlisten") {
          eventListeners.delete(args.eventId);
          return null;
        }
        return null;
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  });
  return errors;
}
