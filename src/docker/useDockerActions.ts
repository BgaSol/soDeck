import { useEffect, useRef, useState } from "react";
import { useDockerManagement } from "../DockerManagementProvider";
import {
  dockerApi,
  operationBlocks,
  type DockerActionPayload,
  type DockerOperationRequest,
  type DockerPreview,
} from "../dockerApi";
import { errorOf } from "../types";

/** Shared submission path: a lost IPC response is retried only with the same id. */
export function useDockerActions(scope: string, enabled = true) {
  const { store, state } = useDockerManagement();
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const [preview, setPreview] = useState<DockerPreview | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const request = useRef<DockerOperationRequest | null>(null),
    flight = useRef(false);
  const current = useRef(scope);
  current.current = scope;
  useEffect(() => {
    setPreview(null);
    setError("");
    setPending(null);
    setBusy(false);
    request.current = null;
  }, [scope]);
  useEffect(() => {
    if (pending && state.operations.some((t) => t.id === pending))
      setPending(null);
    if (
      request.current &&
      state.operations.some((t) => t.requestId === request.current!.requestId)
    ) {
      request.current = null;
      setError("");
    }
  }, [state.operations, pending]);
  async function submit(
    targetId: string,
    action: DockerActionPayload,
    confirmation: DockerPreview | null = null,
  ) {
    if (!state.engine || !enabled) throw new Error("当前引擎不可用");
    const origin = scope;
    request.current ??= {
      requestId: crypto.randomUUID(),
      engineToken: state.engine.token,
      targetId,
      action,
      previewId: confirmation?.id ?? null,
    };
    try {
      const id = await dockerApi.submit(store.sessionId, request.current);
      if (current.current === origin) {
        request.current = null;
        setPending(id);
      }
    } catch (e) {
      const failure = errorOf(e);
      if (
        [
          "validation",
          "conflict",
          "permission",
          "busy",
          "limit",
          "confirmationRequired",
          "disconnected",
          "cancelled",
          "unavailable",
        ].includes(failure.kind)
      )
        request.current = null;
      if (current.current === origin) setError(failure.message);
      throw e;
    }
  }
  async function act(
    targetId: string,
    action: DockerActionPayload,
    confirm = true,
  ) {
    if (
      !enabled ||
      !state.engine ||
      flight.current ||
      pending ||
      request.current
    )
      return;
    const origin = scope;
    flight.current = true;
    setBusy(true);
    setError("");
    try {
      if (confirm) {
        const value = await dockerApi.prepare(
          store.sessionId,
          state.engine.token,
          targetId,
          action,
        );
        if (current.current === origin) setPreview(value);
      } else await submit(targetId, action);
    } catch (e) {
      if (current.current === origin) setError(errorOf(e).message);
      throw e;
    } finally {
      flight.current = false;
      if (current.current === origin) setBusy(false);
    }
  }
  const locked = state.operations.some(
    (t) => t.engine.endpoint === state.engine?.endpoint && operationBlocks(t),
  );
  return {
    busy: busy || !!pending,
    error,
    preview,
    setPreview,
    act,
    submit,
    locked,
    uncertain: !!request.current,
    retry: async () => {
      const r = request.current;
      if (!r || flight.current) return;
      flight.current = true;
      setBusy(true);
      try {
        await submit(r.targetId, r.action);
      } finally {
        flight.current = false;
        setBusy(false);
      }
    },
  };
}
