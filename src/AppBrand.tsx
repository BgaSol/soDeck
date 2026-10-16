import { useEffect, useRef, useState } from "react";
import { DialogHeader } from "./DialogHeader";
import { APP_DESCRIPTION, APP_ICON, APP_NAME, APP_VERSION } from "./brand";

export function BrandIcon({ size = 34 }: { size?: number }) {
  return (
    <img
      className="brand-icon"
      src={APP_ICON}
      width={size}
      height={size}
      alt=""
      aria-hidden="true"
      draggable={false}
    />
  );
}

function AboutDialog({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const modal = dialog.current!;
    modal.showModal();
    return () => modal.close();
  }, []);
  function close() {
    dialog.current?.close();
    onClose();
  }
  return (
    <dialog
      ref={dialog}
      className="about-dialog"
      aria-labelledby="about-title"
      onCancel={(event) => {
        event.preventDefault();
        close();
      }}
    >
      <DialogHeader
        id="about-title"
        title={`关于 ${APP_NAME}`}
        closeLabel="关闭关于窗口"
        onClose={close}
      />
      <div className="about-identity">
        <BrandIcon size={80} />
        <strong>{APP_NAME}</strong>
        <p>你的{APP_DESCRIPTION}</p>
        <span className="about-version">版本 {APP_VERSION}</span>
      </div>
      <p className="about-description">
        在同一个工作台中连接服务器、管理远程文件和 Docker 服务。
      </p>
      <footer className="dialog-actions">
        <button className="primary" onClick={close}>
          关闭
        </button>
      </footer>
    </dialog>
  );
}

export function AppBrand({
  compact = false,
  iconOnly = false,
}: {
  compact?: boolean;
  iconOnly?: boolean;
}) {
  const [aboutOpen, setAboutOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        ref={trigger}
        className={`brand-button${compact || iconOnly ? " brand-compact" : ""}${iconOnly ? " brand-icon-only" : ""}`}
        aria-label={`关于 ${APP_NAME}`}
        aria-haspopup="dialog"
        title={`关于 ${APP_NAME}`}
        onClick={() => setAboutOpen(true)}
      >
        <BrandIcon size={compact || iconOnly ? 24 : 34} />
        {!iconOnly && (
          <span className="app-brand">
            <strong>{APP_NAME}</strong>
            {!compact && <span>{APP_DESCRIPTION}</span>}
          </span>
        )}
      </button>
      {aboutOpen && (
        <AboutDialog
          onClose={() => {
            setAboutOpen(false);
            trigger.current?.focus();
          }}
        />
      )}
    </>
  );
}
