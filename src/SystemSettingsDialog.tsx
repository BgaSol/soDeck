import { DiagnosticsSection } from "./DiagnosticsSection";
import { listen } from "@tauri-apps/api/event";
import { SoftwareUpdateSection } from "./SoftwareUpdateSection";
import {
  useSystemSettings,
  receiveSystemSettings,
  refreshSystemSettings,
} from "./useSystemSettings";
import { convenientKeys } from "./terminalClipboard/policy";
import type { TerminalClipboardPreferences } from "./terminalClipboard/types";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Settings, SquareTerminal } from "lucide-react";
import { desktop } from "./api";
import { DialogHeader } from "./DialogHeader";
import { systemSettingsApi } from "./systemSettingsApi";
import { errorOf } from "./types";
import "./system-settings.css";
import "./terminalClipboard/terminal-clipboard.css";
import {
  getThemePreference,
  setThemePreference,
  subscribeTheme,
  themeOptions,
  type ThemePreference,
} from "./theme";

export function SystemSettingsButton() {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!desktop) return;
    let gone = false;
    let off: (() => void) | undefined;
    void listen("open-software-update", () => setOpen(true)).then(fn => {
      if (gone) fn();
      else off = fn;
    }).catch(() => {});
    return () => { gone = true; off?.(); };
  }, []);
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

export function SystemSettingsDialog({
  onClose,
  section,
}: {
  onClose: () => void;
  section?: "terminal";
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const terminalSection = useRef<HTMLElement>(null);
  const settings = useSystemSettings();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState<boolean | null>(null);
  const [opening, setOpening] = useState(false);
  const [saved, setSaved] = useState(false);
  const [clipboardSaved, setClipboardSaved] = useState(false);
  const [, setThemeRevision] = useState(0);
  const saving = useRef(false);
  const alive = useRef(true);
  const receive = receiveSystemSettings;
  useEffect(() => {
    const off = subscribeTheme(() => setThemeRevision((v) => v + 1));
    return () => {
      off();
    };
  }, []);
  useEffect(() => {
    alive.current = true;
    dialog.current?.showModal();
    if (section === "terminal") {
      terminalSection.current?.scrollIntoView({ block: "nearest" });
      terminalSection.current?.focus();
    }
    void refreshSystemSettings().catch((e) => {
      if (alive.current) setError(errorOf(e).message);
    });
    const escape = (event: KeyboardEvent) => {
      if (
        event.key === "Escape" &&
        !event.isComposing &&
        Array.from(document.querySelectorAll("dialog[open]")).at(-1) ===
          dialog.current
      ) {
        event.preventDefault();
        event.stopPropagation();
        dialog.current?.close();
        onClose();
      }
    };
    window.addEventListener("keydown", escape, true);
    return () => {
      alive.current = false;
      window.removeEventListener("keydown", escape, true);
    };
  }, []);

  async function saveClipboard(preferences: TerminalClipboardPreferences) {
    if (!settings || saving.current) return;
    saving.current = true;
    setBusy(true);
    setError("");
    setClipboardSaved(false);
    try {
      receive(
        await systemSettingsApi.saveClipboard(preferences, settings.revision),
      );
      if (alive.current) setClipboardSaved(true);
    } catch (error) {
      if (alive.current) setError(errorOf(error).message);
      await refreshSystemSettings().catch(() => {});
    } finally {
      saving.current = false;
      if (alive.current) setBusy(false);
    }
  }

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
        <SoftwareUpdateSection />
        <DiagnosticsSection />
        <section className="appearance-settings" aria-label="外观主题">
          <h3>外观</h3>
          <label>
            主题
            <select
              aria-label="外观主题"
              value={getThemePreference()}
              onChange={(e) =>
                setThemePreference(e.target.value as ThemePreference)
              }
            >
              {themeOptions.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <p>跟随系统时，会根据系统的外观设置自动切换。</p>
        </section>
        <section
          ref={terminalSection}
          className="terminal-clipboard-settings"
          tabIndex={-1}
          aria-label="终端复制粘贴设置"
        >
          <h3>终端 · 复制粘贴</h3>
          {settings?.platform !== "macos" && (
            <>
              <label>
                <input
                  type="checkbox"
                  aria-label="便捷快捷键"
                  disabled={
                    !settings || !!settings.loadError || busy || !desktop
                  }
                  checked={
                    settings
                      ? convenientKeys(
                          settings.platform,
                          settings.terminalClipboard,
                        )
                      : false
                  }
                  onChange={(event) =>
                    settings &&
                    void saveClipboard({
                      ...settings.terminalClipboard,
                      shortcutMode: event.target.checked
                        ? "convenient"
                        : "traditional",
                    })
                  }
                />
                便捷快捷键
              </label>
              <p>
                有选区时 Ctrl+C 复制，Ctrl+V 粘贴。关闭后使用 Ctrl+Shift+C /
                V，Ctrl+C / V 交给远端。
              </p>
            </>
          )}
          {settings?.platform === "macos" && (
            <p>使用 ⌘C / ⌘V 复制粘贴，Ctrl 按键交给远端。</p>
          )}
          <label>
            <input
              type="checkbox"
              aria-label="选中即复制"
              disabled={!settings || !!settings.loadError || busy || !desktop}
              checked={settings?.terminalClipboard.copyOnSelect ?? false}
              onChange={(event) =>
                settings &&
                void saveClipboard({
                  ...settings.terminalClipboard,
                  copyOnSelect: event.target.checked,
                })
              }
            />
            选中即复制
          </label>
          <p>完成鼠标选区后，将所选文字复制到系统剪贴板。</p>
          <label>
            <input
              type="checkbox"
              aria-label="右键直接粘贴"
              disabled={!settings || !!settings.loadError || busy || !desktop}
              checked={settings?.terminalClipboard.rightClickPaste ?? false}
              onChange={(event) =>
                settings &&
                void saveClipboard({
                  ...settings.terminalClipboard,
                  rightClickPaste: event.target.checked,
                })
              }
            />
            右键直接粘贴
          </label>
          <p>
            开启后用 Shift+右键打开菜单。远端接管鼠标时，普通右键仍交给远端。
          </p>
          <p role="status">
            {clipboardSaved ? "终端设置已保存，立即生效。" : ""}
          </p>
        </section>
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
