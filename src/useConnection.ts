import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  ConnectionAnswer,
  ConnectionRequest,
  ConnectionView,
  TerminalEvent,
} from "./types";

export type ConnectionPrompt = Extract<
  TerminalEvent,
  { type: "authRequired" | "hostKeyRequired" }
>;

// TerminalPane owns the event channel and closes it on unmount/transfer. This
// hook owns the requested connection, user interaction and displayed progress.
export function useConnection(serverId: string, restored: boolean) {
  const [request, setRequest] = useState<ConnectionRequest | undefined>(() =>
    restored ? undefined : { serverId, attempt: 0 },
  );
  const [prompt, setPrompt] = useState<ConnectionPrompt | null>(null);
  const [progress, setProgress] = useState("");
  const [view, setView] = useState<ConnectionView | null>(null);
  const [connectionId, setConnectionId] = useState<string | null>(null);
  const currentId = useRef<string | null>(null);
  const currentPrompt = useRef<ConnectionPrompt | null>(null);
  function setId(id: string | null) {
    currentId.current = id;
    setConnectionId(id);
  }
  function clearPrompt() {
    currentPrompt.current = null;
    setPrompt(null);
  }
  function receive(event: TerminalEvent) {
    if (event.type === "progress") {
      clearPrompt();
      setProgress(
        `${event.hop.role === "jump" ? "跳板" : "目标"} ${event.hop.server.name} · ${event.stage}`,
      );
    } else if (
      event.type === "authRequired" ||
      event.type === "hostKeyRequired"
    ) {
      currentPrompt.current = event;
      setPrompt(event);
    } else if (event.type === "ready") {
      clearPrompt();
      setProgress("");
      setView(event.connection);
    } else if (event.type === "closed") {
      clearPrompt();
      setProgress("");
      setId(null);
    }
  }
  function reconnect() {
    clearPrompt();
    setProgress("");
    setId(null);
    setView(null);
    setRequest((r) => ({ serverId, attempt: (r?.attempt ?? 0) + 1 }));
  }
  function disconnect() {
    const id = currentId.current;
    clearPrompt();
    setId(null);
    setProgress("");
    setRequest(undefined);
    if (id)
      void invoke("close_connection", { connectionId: id }).catch(() => {});
  }
  async function answer(challengeId: string, response: ConnectionAnswer) {
    const id = currentId.current;
    if (!id || currentPrompt.current?.challenge.id !== challengeId)
      throw new Error("连接或认证提示已结束，请重新连接。");
    await invoke("answer_connection", {
      connectionId: id,
      challengeId,
      response,
    });
    // Progress may already have delivered the next challenge while IPC returned.
    if (
      currentId.current === id &&
      currentPrompt.current?.challenge.id === challengeId
    )
      clearPrompt();
  }
  return {
    request,
    prompt,
    progress,
    view,
    connectionId,
    setId,
    receive,
    reconnect,
    disconnect,
    answer,
  };
}
