import { useState } from "react";
import { actionName, type DockerPreview } from "../dockerApi";
import { DockerDialogFrame } from "./DockerDialogFrame";
import { DockerConfirmationView } from "./DockerConfirmationView";
export function DockerConfirmDialog({
  preview,
  onConfirm,
  onClose,
  onRefresh,
  uncertain = false,
  onRetry,
  disabled = false,
}: {
  preview: DockerPreview;
  onConfirm: () => Promise<void>;
  onClose: () => void;
  onRefresh: () => Promise<void>;
  uncertain?: boolean;
  onRetry?: () => Promise<void>;
  disabled?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <DockerDialogFrame
      title={
        actionName(preview.action) +
        ([
          "start",
          "stop",
          "restart",
          "remove",
          "pause",
          "unpause",
          "kill",
        ].includes(preview.action.type)
          ? "容器？"
          : "？")
      }
      onClose={onClose}
      busy={busy || uncertain}
    >
      <DockerConfirmationView
        preview={preview}
        onBack={onClose}
        onRefresh={onRefresh}
        uncertain={uncertain}
        onRetry={onRetry}
        disabled={disabled}
        onConfirm={async () => {
          setBusy(true);
          try {
            await onConfirm();
            onClose();
          } finally {
            setBusy(false);
          }
        }}
      />
    </DockerDialogFrame>
  );
}
