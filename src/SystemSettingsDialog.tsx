import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Settings, SquareTerminal } from "lucide-react";
import { desktop } from "./api";
import { DialogHeader } from "./DialogHeader";
import { systemSettingsApi, type SystemSettings } from "./systemSettingsApi";
import { errorOf } from "./types";
import "./system-settings.css";

export function SystemSettingsButton() {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        ref={button}
        type="button"
        className="icon-button"
        aria-label="系统设置"
        title="系统设置"
        aria-haspopup="dialog"
        onClick={() => setOpen(true)}
      >
        <Settings size={16} aria-hidden="true" />
      </button>
      {open &&
        createPortal(
          <SystemSettingsDialog
            onClose={() => {
              setOpen(false);
              button.current?.focus();
            }}
          />,
          document.body,
        )}
    </>
  );
}

function SystemSettingsDialog({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [settings, setSettings] = useState<SystemSettings | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState<boolean | null>(null);
  const [opening, setOpening] = useState(false);
  const [saved, setSaved] = useState(false);
  const saving = useRef(false);
  const alive = useRef(true);

  const receive = (next: SystemSettings) => {
    if (alive.current)
      setSettings((current) =>
        !current || next.revision >= current.revision ? next : current,
      );
  };

  useEffect(() => {
    alive.current = true;
    dialog.current?.showModal();
    let unsubscribe: (() => void) | undefined;
    const refresh = () => {
      if (desktop)
        void systemSettingsApi
          .get()
          .then(receive)
          .catch((e) => {
            if (alive.current) setError(errorOf(e).message);
          });
    };
    const escape = (event: KeyboardEvent) => {
      // A disabled focused checkbox can leave focus on the body in Chromium.
      // Keep Escape working while a save is pending, and only close the top dialog.
      if (
        event.key === "Escape" &&
        !event.isComposing &&
        document
          .querySelectorAll("dialog[open]")
          .item(document.querySelectorAll("dialog[open]").length - 1) ===
          dialog.current
      ) {
        event.preventDefault();
        event.stopPropagation();
        dialog.current?.close();
        onClose();
      }
    };
    if (desktop) {
      // Subscribe before taking the snapshot; revisions reject late replies/events.
      void systemSettingsApi
        .subscribe(receive)
        .then((off) => {
          if (!alive.current) off();
          else {
            unsubscribe = off;
            refresh();
          }
        })
        .catch((e) => {
          if (alive.current) {
            setError(errorOf(e).message);
            refresh();
          }
        });
    }
    window.addEventListener("focus", refresh);
    window.addEventListener("keydown", escape, true);
    return () => {
      alive.current = false;
      unsubscribe?.();
      window.removeEventListener("focus", refresh);
      window.removeEventListener("keydown", escape, true);
    };
  }, []);

  async function save(developerMode: boolean) {
    if (!settings || saving.current) return;
    saving.current = true;
    setBusy(true);
    setDraft(developerMode);
    setError("");
    setSaved(false);
    try {
      receive(await systemSettingsApi.save(developerMode, settings.revision));
      if (alive.current) setSaved(true);
    } catch (e) {
      if (alive.current) setError(errorOf(e).message);
      // The checkbox reflects the last committed value, including another window's save.
      try {
        receive(await systemSettingsApi.get());
      } catch {
        /* Keep the last known value. */
      }
    } finally {
      saving.current = false;
      if (alive.current) {
        setBusy(false);
        setDraft(null);
      }
    }
  }

  async function openDevtools() {
    if (opening) return;
    setOpening(true);
    setError("");
    try {
      await systemSettingsApi.openDevtools();
    } catch (e) {
      if (alive.current) setError(errorOf(e).message);
    } finally {
      if (alive.current) setOpening(false);
    }
  }

  const pending =
    settings && settings.developerMode !== settings.activeDeveloperMode;
  function close() {
    dialog.current?.close();
    onClose();
  }
  return (
    <dialog
      ref={dialog}
      className="system-settings-dialog"
      aria-labelledby="system-settings-title"
      onCancel={(event) => {
        event.preventDefault();
        close();
      }}
    >
      <DialogHeader
        id="system-settings-title"
        title="系统设置"
        closeLabel="关闭系统设置"
        onClose={close}
      />
      <div className="system-settings-content">
        <label className="developer-mode-option">
          <input
            type="checkbox"
            checked={draft ?? settings?.developerMode ?? false}
            disabled={!settings || !!settings.loadError || busy || !desktop}
            aria-describedby="developer-mode-description developer-mode-effect"
            aria-labelledby="developer-mode-label"
            onChange={(event) => void save(event.target.checked)}
          />
          <span>
            <strong id="developer-mode-label">开启开发者模式</strong>
            <span id="developer-mode-description">
              允许使用浏览器右键菜单、开发者工具和页面刷新快捷键。
            </span>
          </span>
        </label>
        <p id="developer-mode-effect" className="system-settings-effect">
          更改将在下次启动应用时生效。
        </p>
        <p className="system-settings-current">
          当前状态：
          {settings
            ? settings.activeDeveloperMode
              ? "已开启"
              : "已关闭"
            : desktop
              ? "正在读取…"
              : "仅桌面应用可用"}
        </p>
        <p
          className="system-settings-save-status"
          role="status"
          aria-live="polite"
        >
          {busy
            ? "正在保存…"
            : pending
              ? `已保存，下次启动后${settings.developerMode ? "开启" : "关闭"}开发者模式。`
              : saved
                ? "已保存，与当前状态一致。"
                : ""}
        </p>
        {(error || settings?.loadError) && (
          <p className="panel-error" role="alert">
            {error || settings?.loadError}
          </p>
        )}
        {settings?.activeDeveloperMode && (
          <button
            type="button"
            className="system-settings-devtools"
            disabled={opening}
            onClick={() => void openDevtools()}
          >
            <SquareTerminal size={16} aria-hidden="true" />
            打开当前窗口开发者工具
          </button>
        )}
      </div>
      <footer className="dialog-actions">
        <button type="button" onClick={close}>
          关闭
        </button>
      </footer>
    </dialog>
  );
}
