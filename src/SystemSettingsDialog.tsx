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
import {
  Code2,
  Download,
  Monitor,
  Settings,
  Settings2,
  Stethoscope,
  SquareTerminal,
} from "lucide-react";
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

type SettingsSection =
  "general" | "terminal" | "software-update" | "diagnostics";

const settingsSections = [
  { id: "general", label: "通用", detail: "外观与行为", icon: Settings2 },
  {
    id: "terminal",
    label: "终端",
    detail: "复制与粘贴",
    icon: SquareTerminal,
  },
  {
    id: "software-update",
    label: "软件更新",
    detail: "版本与更新",
    icon: Download,
  },
  {
    id: "diagnostics",
    label: "诊断",
    detail: "日志与支持",
    icon: Stethoscope,
  },
] satisfies ReadonlyArray<{
  id: SettingsSection;
  label: string;
  detail: string;
  icon: typeof Settings2;
}>;

export function SystemSettingsButton() {
  const [open, setOpen] = useState(false);
  const [section, setSection] = useState<SettingsSection>();
  useEffect(() => {
    if (!desktop) return;
    let gone = false;
    let off: (() => void) | undefined;
    void listen("open-software-update", () => {
      setSection("software-update");
      setOpen(true);
    })
      .then((fn) => {
        if (gone) fn();
        else off = fn;
      })
      .catch(() => {});
    return () => {
      gone = true;
      off?.();
    };
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
            section={section}
            onClose={() => {
              setOpen(false);
              setSection(undefined);
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
  section?: SettingsSection;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const activeContent = useRef<HTMLElement>(null);
  const captureActiveContent = (element: HTMLElement | null) => {
    activeContent.current = element;
  };
  const settings = useSystemSettings();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState<boolean | null>(null);
  const [opening, setOpening] = useState(false);
  const [saved, setSaved] = useState(false);
  const [clipboardSaved, setClipboardSaved] = useState(false);
  const [activeSection, setActiveSection] = useState<SettingsSection>(
    section ?? "general",
  );
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

  useEffect(() => {
    if (section) setActiveSection(section);
  }, [section]);

  useEffect(() => {
    activeContent.current?.focus({ preventScroll: true });
  }, [activeSection]);

  const activeMeta =
    settingsSections.find((item) => item.id === activeSection) ??
    settingsSections[0];
  const ActiveIcon = activeMeta.icon;

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
      <div className="system-settings-shell">
        <nav className="system-settings-sidebar" aria-label="设置分类">
          <p className="system-settings-sidebar-title">偏好设置</p>
          <div className="system-settings-nav-list">
            {settingsSections.map(({ id, label, detail, icon: Icon }) => (
              <button
                key={id}
                type="button"
                className={`system-settings-nav-item${activeSection === id ? " is-active" : ""}`}
                aria-current={activeSection === id ? "page" : undefined}
                onClick={() => setActiveSection(id)}
              >
                <span className="system-settings-nav-icon">
                  <Icon size={16} aria-hidden="true" />
                </span>
                <span className="system-settings-nav-copy">
                  <strong>{label}</strong>
                  <small>{detail}</small>
                </span>
              </button>
            ))}
          </div>
          <div className="system-settings-sidebar-foot">
            <Monitor size={14} aria-hidden="true" />
            <span>SoDeck 设置</span>
          </div>
        </nav>
        <main
          className="system-settings-pane"
          aria-labelledby="system-settings-pane-title"
        >
          <header className="system-settings-pane-head">
            <span className="system-settings-pane-icon">
              <ActiveIcon size={20} aria-hidden="true" />
            </span>
            <div>
              <p className="system-settings-eyebrow">设置</p>
              <h3 id="system-settings-pane-title">{activeMeta.label}</h3>
              <p>{activeMeta.detail}</p>
            </div>
          </header>

          {activeSection === "general" && (
            <div
              ref={captureActiveContent}
              className="system-settings-content"
              tabIndex={-1}
            >
              <section
                className="settings-group appearance-settings"
                aria-label="外观主题"
              >
                <div className="settings-group-heading">
                  <div>
                    <h4>外观</h4>
                    <p>选择 SoDeck 在不同环境下的显示方式。</p>
                  </div>
                  <PaletteIcon />
                </div>
                <label className="settings-row settings-select-row">
                  <span>
                    <strong>主题</strong>
                    <small>跟随系统时会自动切换</small>
                  </span>
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
              </section>

              <section className="settings-group" aria-label="开发者工具">
                <div className="settings-group-heading">
                  <div>
                    <h4>开发者工具</h4>
                    <p>为调试和问题排查开放浏览器工具。</p>
                  </div>
                  <Code2 size={18} aria-hidden="true" />
                </div>
                <label className="settings-toggle-row developer-mode-option">
                  <span className="settings-row-copy">
                    <strong id="developer-mode-label">开启开发者模式</strong>
                    <small>允许右键菜单、开发者工具和页面刷新快捷键</small>
                  </span>
                  <span className="settings-switch">
                    <input
                      type="checkbox"
                      checked={draft ?? settings?.developerMode ?? false}
                      disabled={
                        !settings || !!settings.loadError || busy || !desktop
                      }
                      aria-describedby="developer-mode-description developer-mode-effect"
                      aria-labelledby="developer-mode-label"
                      onChange={(event) => void save(event.target.checked)}
                    />
                    <span aria-hidden="true" />
                  </span>
                </label>
                <p id="developer-mode-description" className="settings-help">
                  更改将在下次启动应用时生效。
                </p>
                <p
                  id="developer-mode-effect"
                  className="system-settings-current"
                >
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
                      ? `已保存，下次启动后${settings?.developerMode ? "开启" : "关闭"}开发者模式。`
                      : saved
                        ? "已保存，与当前状态一致。"
                        : ""}
                </p>
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
              </section>
            </div>
          )}

          {activeSection === "software-update" && (
            <div
              ref={captureActiveContent}
              className="system-settings-content"
              tabIndex={-1}
            >
              <SoftwareUpdateSection />
            </div>
          )}
          {activeSection === "diagnostics" && (
            <div
              ref={captureActiveContent}
              className="system-settings-content"
              tabIndex={-1}
            >
              <DiagnosticsSection />
            </div>
          )}
          {activeSection === "terminal" && (
            <section
              ref={captureActiveContent}
              className="system-settings-content terminal-clipboard-settings"
              tabIndex={-1}
              aria-label="终端复制粘贴设置"
            >
              <h3>终端 · 复制粘贴</h3>
              {settings?.platform !== "macos" && (
                <>
                  <label className="settings-toggle-row">
                    <span className="settings-row-copy">
                      <strong>便捷快捷键</strong>
                      <small>有选区时 Ctrl+C 复制，Ctrl+V 粘贴</small>
                    </span>
                    <span className="settings-switch">
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
                      <span aria-hidden="true" />
                    </span>
                  </label>
                  <p className="settings-help">
                    关闭后使用 Ctrl+Shift+C / V，Ctrl+C / V 交给远端。
                  </p>
                </>
              )}
              {settings?.platform === "macos" && (
                <p>使用 ⌘C / ⌘V 复制粘贴，Ctrl 按键交给远端。</p>
              )}
              <label className="settings-toggle-row">
                <span className="settings-row-copy">
                  <strong>选中即复制</strong>
                  <small>完成鼠标选区后，将所选文字复制到系统剪贴板</small>
                </span>
                <span className="settings-switch">
                  <input
                    type="checkbox"
                    aria-label="选中即复制"
                    disabled={
                      !settings || !!settings.loadError || busy || !desktop
                    }
                    checked={settings?.terminalClipboard.copyOnSelect ?? false}
                    onChange={(event) =>
                      settings &&
                      void saveClipboard({
                        ...settings.terminalClipboard,
                        copyOnSelect: event.target.checked,
                      })
                    }
                  />
                  <span aria-hidden="true" />
                </span>
              </label>
              <label className="settings-toggle-row">
                <span className="settings-row-copy">
                  <strong>右键直接粘贴</strong>
                  <small>用 Shift+右键打开菜单，普通右键交给远端</small>
                </span>
                <span className="settings-switch">
                  <input
                    type="checkbox"
                    aria-label="右键直接粘贴"
                    disabled={
                      !settings || !!settings.loadError || busy || !desktop
                    }
                    checked={
                      settings?.terminalClipboard.rightClickPaste ?? false
                    }
                    onChange={(event) =>
                      settings &&
                      void saveClipboard({
                        ...settings.terminalClipboard,
                        rightClickPaste: event.target.checked,
                      })
                    }
                  />
                  <span aria-hidden="true" />
                </span>
              </label>
              <p role="status">
                {clipboardSaved ? "终端设置已保存，立即生效。" : ""}
              </p>
            </section>
          )}
          {(error || settings?.loadError) && (
            <p className="panel-error system-settings-error" role="alert">
              {error || settings?.loadError}
            </p>
          )}
        </main>
      </div>
      <footer className="dialog-actions">
        <button type="button" onClick={close}>
          关闭
        </button>
      </footer>
    </dialog>
  );
}

function PaletteIcon() {
  return (
    <span className="settings-color-icon" aria-hidden="true">
      Aa
    </span>
  );
}
