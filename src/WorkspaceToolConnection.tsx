import {
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type Ref,
} from "react";
import { createPortal } from "react-dom";
import { Channel } from "@tauri-apps/api/core";
import { invokeDiagnosed as invoke } from "./diagnostics";
import { LoginDialog, FingerprintDialog } from "./ConnectionDialogs";
import type { WorkspaceTab } from "./workspaces";
import { workspaceArgs } from "./workspaceApi";
import type { ConnectionPrompt } from "./useConnection";
import { errorOf, type ConnectionAnswer, type TerminalEvent } from "./types";
import type { WorkspaceConnectionQueue } from "./workspaceConnectionQueue";

export interface WorkspaceToolView {
  busy: boolean;
  error: string;
  promptPending: boolean;
}
export interface WorkspaceToolController {
  run: () => void;
}

export function WorkspaceToolConnection({
  workspace,
  active,
  queue,
  controllerRef,
  onState,
}: {
  workspace: WorkspaceTab;
  active: boolean;
  queue: WorkspaceConnectionQueue;
  controllerRef?: Ref<WorkspaceToolController>;
  onState?: (state: WorkspaceToolView) => void;
}) {
  const [prompt, setPrompt] = useState<ConnectionPrompt | null>(null),
    [id, setId] = useState<string | null>(null),
    [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const abort = useRef<AbortController | null>(null);
  const currentId = useRef<string | null>(null);
  const release = useRef<(() => void) | null>(null);
  useEffect(() => {
    onState?.({ busy, error, promptPending: !!prompt });
  }, [onState, busy, error, !!prompt]);
  function cancel() {
    abort.current?.abort();
    if (currentId.current)
      void invoke("cancel_workspace_context", {
        verificationId: currentId.current,
      }).catch(() => {});
    currentId.current = null;
    setId(null);
    setPrompt(null);
    setBusy(false);
  }
  useEffect(
    () => () => {
      abort.current?.abort();
      release.current?.();
      if (currentId.current)
        void invoke("cancel_workspace_context", {
          verificationId: currentId.current,
        }).catch(() => {});
    },
    [],
  );
  useEffect(() => {
    if (active && id && prompt)
      void invoke("present_workspace_context_challenge", {
        verificationId: id,
        challengeId: prompt.challenge.id,
      }).catch(() => {});
  }, [active, id, prompt]);
  async function start() {
    if (busy || (abort.current && !abort.current.signal.aborted)) return;
    setBusy(true);
    setError("");
    const controller = new AbortController();
    abort.current = controller;
    try {
      const done = await queue.acquire(controller.signal);
      release.current = done;
      if (controller.signal.aborted) {
        done();
        return;
      }
      let ended = false;
      const events = new Channel<TerminalEvent>((event) => {
        if (controller.signal.aborted) return;
        if (event.type === "authRequired" || event.type === "hostKeyRequired")
          setPrompt(event);
        if (event.type === "progress") setPrompt(null);
        if (event.type === "closed") {
          ended = true;
          setPrompt(null);
          setBusy(false);
          setId(null);
          currentId.current = null;
          if (abort.current === controller) abort.current = null;
          done();
          if (event.error) setError(event.error.message);
        }
      });
      const verificationId = await invoke<string>("refresh_workspace_context", {
        ...workspaceArgs(workspace),
        events,
      });
      if (controller.signal.aborted) {
        await invoke("cancel_workspace_context", { verificationId });
        done();
      } else if (!ended) {
        currentId.current = verificationId;
        setId(verificationId);
      }
    } catch (e) {
      if (abort.current === controller) {
        if (!controller.signal.aborted) setError(errorOf(e).message);
        release.current?.();
        abort.current = null;
        setBusy(false);
      }
    }
  }
  useImperativeHandle(controllerRef, () => ({
    run: () => {
      if (workspace.lifecycle !== "active") return;
      if (busy) cancel();
      else void start();
    },
  }));
  async function answer(response: ConnectionAnswer) {
    if (!id || !prompt) return;
    const challengeId = prompt.challenge.id;
    await invoke("answer_workspace_context", {
      verificationId: id,
      challengeId,
      response,
    });
    setPrompt((p) => (p?.challenge.id === challengeId ? null : p));
  }
  return (
    <>
      {active &&
        prompt?.type === "authRequired" &&
        createPortal(
          <LoginDialog
            key={prompt.challenge.id}
            challenge={prompt.challenge}
            disabled={!id}
            onSubmit={(c) => answer({ type: "credentials", ...c })}
            onClose={cancel}
          />,
          document.body,
        )}
      {active &&
        prompt?.type === "hostKeyRequired" &&
        createPortal(
          <FingerprintDialog
            key={prompt.challenge.id}
            challenge={prompt.challenge}
            disabled={!id}
            onTrust={() => answer({ type: "trust", accept: true })}
            onClose={cancel}
          />,
          document.body,
        )}
    </>
  );
}
