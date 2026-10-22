import type { Page } from "@playwright/test";

/** Authoritative workspace IPC simulator shared by the existing service fixtures.
 * Terminal and tool capabilities are deliberately different and independently revoked.
 */
export async function installWorkspaceFixture(page: Page) {
  await page.addInitScript(() => {
    const w = window as any;
    let internal: any;
    const wrap = (value: any) => {
      if (!value || value.__workspaces) return value;
      value.__workspaces = true;
      const original = value.invoke.bind(value);
      const callbacks = new Map<number, Function>(),
        listeners = new Map<number, any>();
      const transform = value.transformCallback.bind(value);
      value.transformCallback = (fn: Function, ...args: any[]) => {
        const id = transform(fn, ...args);
        callbacks.set(id, fn);
        return id;
      };
      let revision = 0;
      const workspaces: any[] = [],
        moves = new Map<string, any>(),
        requests = new Map<string, string>();
      const channels = new Map<string, any>();
      const label = () => value.metadata.currentWindow.label;
      const emit = (event: string, payload: any) => {
        for (const [id, l] of listeners)
          if (l.event === event)
            callbacks.get(l.handler)?.({
              id,
              event,
              payload: structuredClone(payload),
            });
      };
      const list = () => ({
        revision,
        workspaces: structuredClone(workspaces),
      });
      const publish = (workspace?: any) => {
        revision++;
        if (workspace) workspace.revision = revision;
        emit("server-workspaces-changed", list());
      };
      const addTerminal = (workspace: any, request: string) => {
        if (requests.has(request)) return;
        const terminal = {
          id: crypto.randomUUID(),
          number: workspace.nextTerminalNumber++,
          title: null,
          attempt: 0,
          connectionId: null,
          state: "queued",
        };
        workspace.terminals.push(terminal);
        workspace.activeTerminalId = terminal.id;
        requests.set(request, terminal.id);
      };
      const owned = (args: any) => {
        const workspace = workspaces.find((t) => t.id === args.workspaceId);
        if (
          !workspace ||
          workspace.owner.label !== label() ||
          workspace.owner.ownerEpoch !== args.ownerEpoch ||
          workspace.lifecycle !== "active"
        )
          throw new Error("workspaceChanged");
        return workspace;
      };
      w.__WORKSPACES__ = { workspaces, moves, list, emit, calls: [] };
      value.invoke = async (command: string, args: any = {}) => {
        w.__WORKSPACES__.calls.push({
          command,
          args: JSON.parse(JSON.stringify(args)),
        });
        if (command === "plugin:event|listen") {
          const id = await original(command, args);
          listeners.set(id, args);
          return id;
        }
        if (command === "plugin:event|unlisten") listeners.delete(args.eventId);
        if (command === "list_server_workspaces") return list();
        if (command === "open_server_workspace") {
          let workspace = workspaces.find((t) => t.server.id === args.serverId);
          if (!workspace) {
            const servers = await original("list_servers");
            workspace = {
              id: crypto.randomUUID(),
              server: servers.find((s: any) => s.id === args.serverId),
              owner: { label: label(), generation: 0, ownerEpoch: 0 },
              revision: 0,
              lifecycle: "active",
              terminals: [],
              activeTerminalId: null,
              nextTerminalNumber: 1,
              tools: {
                context: null,
                state: "unavailable",
                connection: null,
                configurationChanged: false,
              },
            };
            workspaces.push(workspace);
            addTerminal(workspace, args.requestId);
          } else if (args.newTerminal) {
            if (workspace.lifecycle !== "active") throw new Error("moving");
            addTerminal(workspace, args.requestId);
          }
          publish(workspace);
          emit("server-workspace-focus", workspace.id);
          return structuredClone(workspace);
        }
        if (command === "create_workspace_terminal") {
          const workspace = owned(args);
          addTerminal(workspace, args.requestId);
          publish(workspace);
          return structuredClone(workspace);
        }
        if (command === "update_workspace_terminal") {
          const workspace = owned(args),
            update = args.update;
          const terminal = workspace.terminals.find(
            (t: any) => t.id === update.id,
          );
          if (update.type === "select")
            workspace.activeTerminalId = terminal.id;
          if (update.type === "rename") terminal.title = update.title;
          if (update.type === "report" && terminal.attempt === update.attempt)
            terminal.state = update.state;
          if (update.type === "reorder")
            workspace.terminals.sort(
              (a: any, b: any) =>
                update.ids.indexOf(a.id) - update.ids.indexOf(b.id),
            );
          publish(workspace);
          return structuredClone(workspace);
        }
        if (command === "open_connection") {
          const workspace = owned(args.binding),
            terminal = workspace.terminals.find(
              (t: any) => t.id === args.binding.terminalId,
            );
          if (!terminal) throw new Error("foreign terminal");
          const handler = args.events.onmessage;
          terminal.attempt = args.binding.attempt;
          terminal.state = "connecting";
          let closed = false;
          args.events.onmessage = (event: any) => {
            if (closed) {
              handler(event);
              return;
            }
            if (event.type === "ready") {
              terminal.state = "connected";
              if (!workspace.tools.context)
                workspace.tools = {
                  context: { id: `tools-${workspace.id}.1`, epoch: 1 },
                  state: "available",
                  connection: event.connection,
                  configurationChanged: false,
                };
            }
            if (event.type === "closed") {
              closed = true;
              terminal.connectionId = null;
              terminal.state =
                event.reason === "failed" ? "failed" : "disconnected";
            }
            if (
              event.type === "authRequired" ||
              event.type === "hostKeyRequired"
            )
              terminal.state = "authenticating";
            if (event.type !== "data" && event.type !== "cwd")
              publish(workspace);
            handler(event);
          };
          const id = await original(command, args);
          if (!closed) terminal.connectionId = id;
          channels.set(id, args.events);
          publish(workspace);
          return id;
        }
        if (command === "prepare_workspace_close") {
          const workspace = owned(args);
          return {
            revision: workspace.revision,
            terminals: workspace.terminals.length,
            connected: workspace.terminals.filter((t: any) => t.connectionId)
              .length,
            verifying: false,
            blockers: [],
            independentTasks: 0,
          };
        }
        if (command === "close_server_workspace") {
          const workspace = owned(args);
          if (workspace.revision !== args.revision)
            throw new Error("workspaceChanged");
          workspaces.splice(workspaces.indexOf(workspace), 1);
          for (const terminal of workspace.terminals)
            if (terminal.connectionId)
              await original("close_connection", {
                connectionId: terminal.connectionId,
              });
          publish();
          return;
        }
        if (command === "close_workspace_terminal") {
          const workspace = owned(args),
            terminal = workspace.terminals.find(
              (t: any) => t.id === args.terminalId,
            );
          workspace.terminals = workspace.terminals.filter(
            (t: any) => t.id !== args.terminalId,
          );
          if (workspace.activeTerminalId === args.terminalId)
            workspace.activeTerminalId = workspace.terminals.at(-1)?.id ?? null;
          if (terminal?.connectionId)
            await original("close_connection", {
              connectionId: terminal.connectionId,
            });
          publish(workspace);
          return;
        }
        if (command === "prepare_workspace_move") {
          const workspace = owned(args);
          workspace.lifecycle = "moving";
          publish(workspace);
          const transferId = crypto.randomUUID();
          moves.set(transferId, { workspace, acknowledgements: new Set() });
          return { transferId, workspace: structuredClone(workspace) };
        }
        if (command === "move_workspace") {
          await original(command, args);
          return args.payload.transferId;
        }
        if (command === "take_workspace") {
          const incoming = await original(command, args),
            snapshot = incoming.payload;
          if (snapshot.version !== 2) throw new Error("snapshot version");
          const workspace = structuredClone(snapshot.tab);
          workspace.owner = {
            label: label(),
            generation: 0,
            ownerEpoch: workspace.owner.ownerEpoch + 1,
          };
          workspace.lifecycle = "moving";
          workspaces.push(workspace);
          moves.set(args.transferId, {
            workspace,
            acknowledgements: new Set(),
          });
          return {
            ...incoming,
            payload: {
              ...snapshot,
              transferId: args.transferId,
              tab: workspace,
            },
          };
        }
        if (command === "terminal_attach") {
          const result = await original(command, args);
          const entry = [...moves.entries()].find(([, m]) =>
            m.workspace.terminals.some(
              (t: any) => t.connectionId === args.sessionId,
            ),
          );
          if (!entry) throw new Error("missing member");
          args.output.onmessage({
            type: "receptionBarrier",
            transferId: entry[0],
            sessionId: args.sessionId,
          });
          return result;
        }
        if (command === "acknowledge_workspace_terminal") {
          const m = moves.get(args.transferId);
          if (
            !m?.workspace.terminals.some((t: any) => t.id === args.terminalId)
          )
            throw new Error("foreign member");
          m.acknowledgements.add(args.terminalId);
          return;
        }
        if (command === "workspace_ready") {
          const m = moves.get(args.transferId);
          if (!m || m.acknowledgements.size !== m.workspace.terminals.length)
            throw new Error("incomplete reception");
          await original(command, args);
          m.workspace.lifecycle = "active";
          publish(m.workspace);
          return;
        }
        if (command === "cancel_workspace_move") {
          const m = moves.get(args.transferId);
          if (m) {
            m.workspace.lifecycle = "active";
            m.workspace.owner.ownerEpoch++;
            publish(m.workspace);
            moves.delete(args.transferId);
          }
          return original(command, args);
        }
        if (command === "refresh_workspace_context") {
          const workspace = owned(args);
          const epoch = (workspace.tools.context?.epoch ?? 0) + 1;
          const id = crypto.randomUUID();
          setTimeout(() => {
            workspace.tools.context = {
              id: "tools-" + crypto.randomUUID() + "." + epoch,
              epoch,
            };
            workspace.tools.state = "available";
            publish(workspace);
            args.events.onmessage({
              type: "closed",
              reason: "ended",
              message: "verified",
              error: null,
              recovery: "none",
            });
          }, 0);
          return id;
        }
        if (command === "present_connection_challenge") return;
        if (args.context) {
          const workspace = workspaces.find(
            (t) =>
              t.tools.context?.id === args.context.id &&
              t.tools.context.epoch === args.context.epoch,
          );
          if (
            !workspace ||
            workspace.lifecycle !== "active" ||
            workspace.owner.label !== label()
          )
            throw new Error("stale tool capability");
          // Legacy service fixtures use this field as a map key, never as a live terminal.
          return original(command, { ...args, sessionId: args.context.id });
        }
        return original(command, args);
      };
      return value;
    };
    internal = wrap(w.__TAURI_INTERNALS__);
    Object.defineProperty(w, "__TAURI_INTERNALS__", {
      configurable: true,
      get: () => internal,
      set: (v) => {
        internal = wrap(v);
      },
    });
  });
}
