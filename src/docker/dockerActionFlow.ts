import type { DockerOperationRequest, DockerPreview } from "../dockerApi";
export type DockerActionPhase =
  | "idle"
  | "preparing"
  | "confirming"
  | "refreshing"
  | "submitting"
  | "awaitingResult"
  | "accepted";
export function submissionRequest(
  preview: DockerPreview,
): DockerOperationRequest {
  return {
    requestId: crypto.randomUUID(),
    engineToken: preview.engine.token,
    targetId: preview.targetId,
    action: structuredClone(preview.action),
    previewId: preview.id,
  };
}
export function definitelyRejected(kind: string) {
  return [
    "validation",
    "conflict",
    "permission",
    "busy",
    "limit",
    "confirmationRequired",
    "disconnected",
    "cancelled",
    "unavailable",
    "previewUnavailable",
  ].includes(kind);
}
export function previewExpired(preview: DockerPreview, now = Date.now()) {
  return now >= preview.expiresAt;
}
