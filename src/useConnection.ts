import { useEffect, useRef, useState } from "react";
import { invokeDiagnosed as invoke } from "./diagnostics";
import {
  ConnectionRecovery,
  initialRecovery,
  type RecoverySnapshot,
} from "./connectionRecovery";
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

// TerminalPane owns the channel; this hook is the sole retry scheduler.
export function useConnection(
  serverId: string,
  options: {
    restored: boolean;
    connected?: boolean;
    recovery?: RecoverySnapshot;
    onAttempt: () => void;
  },
) {
  const [request, setRequest] = useState<ConnectionRequest | undefined>(() =>
    options.restored ? undefined : { serverId, attempt: 0 },
  );
  const [prompt, setPrompt] = useState<ConnectionPrompt | null>(null);
  const [progress, setProgress] = useState("");
  const [view, setView] = useState<ConnectionView | null>(null);
  const [connectionId, setConnectionId] = useState<string | null>(null);
  const [recovery, setRecovery] = useState<RecoverySnapshot>(
    () =>
      options.recovery ?? {
        ...initialRecovery(options.connected),
        phase: options.restored ? "idle" : "connecting",
      },
  );
  const [now, setNow] = useState(Date.now());
  const currentId = useRef<string | null>(null);
  const currentPrompt = useRef<ConnectionPrompt | null>(null);
  const generation = useRef(0);
  const onAttempt = useRef(options.onAttempt);
  onAttempt.current = options.onAttempt;
  const schedulerRef = useRef<ConnectionRecovery | null>(null);
  function setId(id: string | null) {
    currentId.current = id;
    setConnectionId(id);
  }
  function clearPrompt() {
    currentPrompt.current = null;
    setPrompt(null);
  }
  if (!schedulerRef.current)
    schedulerRef.current = new ConnectionRecovery(recovery, setRecovery, () => {
      generation.current++;
      clearPrompt();
      setProgress("");
      setId(null);
      setView(null);
      onAttempt.current();
      setRequest({
        serverId,
        attempt: generation.current,
        automatic: schedulerRef.current!.state.automatic,
      });
    });
  const scheduler = schedulerRef.current;
  useEffect(() => {
    // Restored workspaces resume after the backend commits the move.
    if (!options.restored) scheduler.resume();
    return () => {
      scheduler.pause();
    };
  }, [scheduler, options.restored]);
  useEffect(() => {
    if (recovery.phase !== "waiting") return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(timer);
  }, [recovery.phase, recovery.nextRetryAt]);
  function receive(event: TerminalEvent, attempt = generation.current) {
    if (attempt !== generation.current || scheduler.state.phase === "stopped")
      return;
    if (event.type === "progress") {
      clearPrompt();
      scheduler.connecting();
      setProgress(
        (event.hop.role === "jump" ? "跳板" : "目标") +
          " " +
          event.hop.server.name +
          " · " +
          event.stage,
      );
    } else if (
      event.type === "authRequired" ||
      event.type === "hostKeyRequired"
    ) {
      scheduler.prompt();
      currentPrompt.current = event;
      setPrompt(event);
    } else if (event.type === "ready") {
      clearPrompt();
      setProgress("");
      setView(event.connection);
      scheduler.connected();
    } else if (event.type === "closed") {
      clearPrompt();
      setProgress("");
      setId(null);
      scheduler.closed(event);
    }
  }
  function reconnect() {
    scheduler.start(scheduler.state.phase === "waiting");
  }
  function disconnect() {
    generation.current++;
    scheduler.stop();
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
    if (
      currentId.current === id &&
      currentPrompt.current?.challenge.id === challengeId
    ) {
      clearPrompt();
      scheduler.connecting();
    }
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
    recovery,
    seconds: Math.max(
      0,
      Math.ceil(((recovery.nextRetryAt ?? now) - now) / 1000),
    ),
    pauseRecovery: () => scheduler.pause(),
    resumeRecovery: () => scheduler.resume(),
  };
}
