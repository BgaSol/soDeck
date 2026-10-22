import { useWorkspaceDialog } from "../WorkspaceInteractionContext";
import { createPortal } from "react-dom";
import {
  useEffect,
  useId,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";
import { DialogHeader } from "../DialogHeader";
export function DockerDialogFrame({
  title,
  children,
  footer,
  onClose,
  busy = false,
  wide = false,
  dialogRef,
  onSubmit,
  focusKey = "initial",
}: {
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  onClose: () => void;
  busy?: boolean;
  wide?: boolean;
  dialogRef?: RefObject<HTMLDialogElement | null>;
  onSubmit?: () => void;
  focusKey?: string;
}) {
  const local = useRef<HTMLDialogElement>(null),
    ref = dialogRef ?? local,
    id = useId();
  useWorkspaceDialog(ref, "services", true, "docker");
  useEffect(() => {
    if (!ref.current?.open) return;
    const target =
      ref.current?.querySelector<HTMLElement>(
        "[data-initial-focus]:not([hidden])",
      ) ??
      ref.current?.querySelector<HTMLElement>(
        ".docker-flow-body input, .docker-flow-body select, .docker-flow-body textarea",
      );
    target?.focus({ preventScroll: true });
  }, [focusKey, ref]);
  return createPortal(
    <dialog
      ref={ref}
      className={"docker-flow-dialog" + (wide ? " docker-flow-wide" : "")}
      aria-labelledby={id}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <DialogHeader
        id={id}
        title={title}
        closeLabel={"关闭" + title}
        onClose={onClose}
        disabled={busy}
      />
      <form
        className="docker-flow-form"
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
          onSubmit?.();
        }}
      >
        {footer === undefined ? (
          children
        ) : (
          <>
            <div className="docker-flow-body">{children}</div>
            <footer className="dialog-actions">{footer}</footer>
          </>
        )}
      </form>
    </dialog>,
    document.body,
  );
}
