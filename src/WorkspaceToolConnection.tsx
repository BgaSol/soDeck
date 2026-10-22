import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Channel } from "@tauri-apps/api/core";
import { invokeDiagnosed as invoke } from "./diagnostics";
import { LoginDialog, FingerprintDialog } from "./ConnectionDialogs";
import type { WorkspaceTab } from "./workspaces";
import { workspaceArgs } from "./workspaceApi";
import type { ConnectionPrompt } from "./useConnection";
import { errorOf, type ConnectionAnswer, type TerminalEvent } from "./types";
import type { WorkspaceConnectionQueue } from "./workspaceConnectionQueue";

export function WorkspaceToolConnection({
  workspace,
  active,
  queue,
}: {
  workspace: WorkspaceTab;
  active: boolean;
  queue: WorkspaceConnectionQueue;
}) {
  const [prompt, setPrompt] = useState<ConnectionPrompt | null>(null),
    [id, setId] = useState<string | null>(null),
    [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const abort = useRef<AbortController | null>(null);
  const currentId = useRef<string | null>(null);
  const release = useRef<(() => void) | null>(null);
  function cancel() {
    abort.current?.abort();
    if (currentId.current)
      void invoke("cancel_workspace_context", {
        verificationId: currentId.current,
      }).catch(() => {});
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
    if (busy) return;
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
      if (!controller.signal.aborted) setError(errorOf(e).message);
      release.current?.();
      setBusy(false);
    }
  }
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
      <span>
        {busy
          ? "正在验证工具连接"
          : workspace.tools.context
            ? "工具可用"
            : "工具需要验证"}
      </span>
      {workspace.tools.configurationChanged && (
        <span>配置已更新，工具仍使用原连接</span>
      )}
      <button
        className="quiet"
        disabled={workspace.lifecycle !== "active"}
        onClick={() => (busy ? cancel() : void start())}
      >
        {busy
          ? "取消验证"
          : workspace.tools.context
            ? "更新工具连接"
            : "连接工具"}
      </button>
      {error && <span role="alert">{error}</span>}
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
