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
import {
  definitelyRejected,
  previewExpired,
  submissionRequest,
  type DockerActionPhase,
} from "./dockerActionFlow";

/** A request is frozen on first submission and retained until acceptance is known. */
export function useDockerActions(
  scope: string,
  enabled = true,
  onAccepted?: () => void,
) {
  const { store, state } = useDockerManagement();
  const [phase, setPhase] = useState<DockerActionPhase>("idle"),
    [error, setError] = useState("");
  const [preview, updatePreview] = useState<DockerPreview | null>(null),
    [pending, setPending] = useState<string | null>(null);
  const request = useRef<DockerOperationRequest | null>(null),
    flight = useRef(false),
    epoch = useRef(0);
  const current = useRef(scope),
    livePreview = useRef<DockerPreview | null>(null),
    accepted = useRef(new Set<string>());
  const callback = useRef(onAccepted);
  callback.current = onAccepted;
  current.current = scope;
  const release = (p: DockerPreview | null) => {
    if (p)
      void dockerApi
        .releasePreview(store.contextId, p.draftToken)
        .catch(() => {});
  };
  useEffect(() => {
    ++epoch.current;
    flight.current = false;
    request.current = null;
    updatePreview(null);
    setError("");
    setPending(null);
    setPhase("idle");
    return () => {
      ++epoch.current;
      release(livePreview.current);
      livePreview.current = null;
    };
  }, [scope, store]);
  function accept(r: DockerOperationRequest, id: string) {
    if (accepted.current.has(r.requestId)) return;
    accepted.current.add(r.requestId);
    request.current = null;
    setPending(id);
    setPhase("accepted");
    setError("");
    release(livePreview.current);
    livePreview.current = null;
    updatePreview(null);
    callback.current?.();
  }
  useEffect(() => {
    if (pending && state.operations.some((t) => t.id === pending))
      setPending(null);
    const r = request.current;
    const found =
      r && state.operations.find((t) => t.requestId === r.requestId);
    if (r && found) accept(r, found.id);
  }, [state.operations, pending]);
  function setPreview(value: DockerPreview | null) {
    if (request.current) return;
    if (livePreview.current?.draftToken !== value?.draftToken)
      release(livePreview.current);
    livePreview.current = value;
    updatePreview(value);
    setPhase(value ? "confirming" : "idle");
    setError("");
  }
  async function prepareWith(prepare: () => Promise<DockerPreview | null>) {
    if (!enabled || flight.current || pending || request.current) return;
    const origin = scope,
      generation = ++epoch.current;
    flight.current = true;
    setPhase("preparing");
    setError("");
    try {
      const value = await prepare();
      if (current.current !== origin || epoch.current !== generation) {
        release(value);
        return;
      }
      setPreview(value);
    } catch (e) {
      if (current.current === origin && epoch.current === generation) {
        setError(errorOf(e).message);
        setPhase("idle");
      }
      throw e;
    } finally {
      if (epoch.current === generation) flight.current = false;
    }
  }
  async function renewPreview() {
    const p = livePreview.current;
    if (!p || flight.current || request.current) return;
    const origin = scope,
      generation = ++epoch.current;
    flight.current = true;
    setPhase("refreshing");
    setError("");
    try {
      const value = await dockerApi.renewPreview(
        store.contextId,
        p.draftToken,
        p.id,
      );
      if (current.current !== origin || epoch.current !== generation) {
        release(value);
        return;
      }
      setPreview(value);
    } catch (e) {
      if (current.current === origin && epoch.current === generation) {
        setError(errorOf(e).message);
        setPhase("confirming");
      }
      throw e;
    } finally {
      if (epoch.current === generation) flight.current = false;
    }
  }
  async function submit(
    targetId: string,
    action: DockerActionPayload,
    confirmation: DockerPreview | null = null,
  ) {
    if (flight.current) return;
    if (!request.current && (!state.engine || !enabled))
      throw new Error("当前引擎不可用");
    if (!request.current && confirmation && previewExpired(confirmation))
      throw new Error("确认已过期，请重新检查。");
    const r =
      request.current ??
      (confirmation
        ? submissionRequest(confirmation)
        : {
            requestId: crypto.randomUUID(),
            engineToken: state.engine!.token,
            targetId,
            action: structuredClone(action),
            previewId: null,
          });
    request.current = r;
    const origin = scope,
      generation = epoch.current;
    flight.current = true;
    setPhase("submitting");
    setError("");
    try {
      const id = await dockerApi.submit(store.contextId, r);
      if (current.current === origin && epoch.current === generation)
        accept(r, id);
    } catch (e) {
      if (accepted.current.has(r.requestId)) return;
      if (current.current === origin && epoch.current === generation) {
        const failure = errorOf(e);
        if (definitelyRejected(failure.kind)) request.current = null;
        setPhase(
          request.current
            ? "awaitingResult"
            : livePreview.current
              ? "confirming"
              : "idle",
        );
        setError(failure.message);
      }
      throw e;
    } finally {
      if (epoch.current === generation) flight.current = false;
    }
  }
  async function act(
    targetId: string,
    action: DockerActionPayload,
    confirm = true,
  ) {
    if (request.current) throw new Error("请先核对原提交结果");
    if (
      !state.engine ||
      !enabled ||
      flight.current ||
      pending ||
      request.current
    )
      return;
    if (confirm)
      await prepareWith(() =>
        dockerApi.prepare(
          store.contextId,
          state.engine!.token,
          targetId,
          action,
        ),
      );
    else await submit(targetId, action);
  }
  const busy = ["preparing", "refreshing", "submitting"].includes(phase);
  return {
    phase,
    busy: busy || !!pending,
    error,
    preview,
    setPreview,
    act,
    submit,
    prepareWith,
    renewPreview,
    locked: state.operations.some(
      (t) => t.engine.endpoint === state.engine?.endpoint && operationBlocks(t),
    ),
    uncertain: phase === "awaitingResult",
    retry: async () => {
      const r = request.current;
      if (r) await submit(r.targetId, r.action);
    },
    dismissPreview: () => {
      if (request.current) return;
      ++epoch.current;
      flight.current = false;
      setPreview(null);
    },
  };
}
