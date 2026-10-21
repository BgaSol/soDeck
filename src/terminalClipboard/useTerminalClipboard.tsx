import { useEffect, useRef, useState } from "react";
import { recordDiagnostic } from "../diagnostics";
import { createPortal } from "react-dom";
import type { Terminal } from "@xterm/xterm";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { desktop } from "../api";
import { useSystemSettings, browserPlatform } from "../useSystemSettings";
import { SystemSettingsDialog } from "../SystemSettingsDialog";
import { copyText, textClipboard } from "./clipboardService";
import {
  TerminalClipboardController,
  type ClipboardEnvironment,
} from "./controller";
import {
  defaultPreferences,
  emptyClipboardState,
  type ClipboardViewState,
} from "./types";
import { resolveKeyAction, shortcutLabels } from "./policy";
import { TerminalContextMenu } from "./TerminalContextMenu";
import { TerminalPasteDialog } from "./TerminalPasteDialog";
import "./terminal-clipboard.css";

type BindingEnvironment = Pick<
  ClipboardEnvironment,
  "target" | "reason" | "paste" | "control"
> & {
  invalidate(): void;
  canFocus(): boolean;
};
export function useTerminalClipboard(
  onState?: (state: ClipboardViewState) => void,
) {
  const settings = useSystemSettings();
  const preferences = settings?.terminalClipboard ?? defaultPreferences;
  const platform = settings?.platform ?? browserPlatform;
  const options = useRef({ preferences, platform });
  options.current = { preferences, platform };
  const stateCallback = useRef(onState);
  stateCallback.current = onState;
  const [state, setState] = useState<ClipboardViewState>(emptyClipboardState);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const modalOpen = useRef(false);
  modalOpen.current = settingsOpen;
  const owner = useRef(crypto.randomUUID()).current;
  const controller = useRef<TerminalClipboardController | null>(null);
  const binding = useRef<{
    term: Terminal;
    env: BindingEnvironment;
    readonly: boolean;
    invalidate(): void;
    focus(): void;
  } | null>(null);
  const change = (next: ClipboardViewState) => {
    if (next.notice && next.notice !== state.notice) recordDiagnostic({ event: "clipboard.result", outcome: next.notice.kind === "error" ? "failed" : "ok" });
    setState(next);
    stateCallback.current?.(next);
  };
  function attach(term: Terminal, env: BindingEnvironment, readonly = false) {
    let disposed = false;
    let focused = document.hasFocus();
    let composing = false;
    let selecting = false;
    let rightAction: "menu" | "paste" | null = null;
    let focusIntent = 0;
    let focusFrame: number | undefined;
    let automaticCopyPending = false;
    let nextAutomaticCopy: string | undefined;
    const host = term.element!;
    const modalAllowed = () =>
      !modalOpen.current &&
      Array.from(
        document.querySelectorAll<HTMLDialogElement>("dialog[open]"),
      ).every((dialog) => dialog.dataset.clipboardOwner === owner);
    const target = () =>
      focused &&
      document.visibilityState !== "hidden" &&
      modalAllowed() &&
      !readonly
        ? env.target()
        : null;
    const focus = () => {
      if (disposed || !focused || !modalAllowed() || !env.canFocus()) return;
      const ownDialog = Array.from(
        document.querySelectorAll<HTMLDialogElement>("dialog[open]"),
      ).some((dialog) => dialog.dataset.clipboardOwner === owner);
      if (!ownDialog) {
        term.focus();
        return;
      }
      // React must remove the modal before the terminal can receive focus. A new
      // user action or session transition cancels this deferred restoration.
      const epoch = env.target()?.epoch;
      const intent = focusIntent;
      if (focusFrame !== undefined) cancelAnimationFrame(focusFrame);
      focusFrame = requestAnimationFrame(() => {
        focusFrame = undefined;
        if (
          !disposed &&
          focused &&
          intent === focusIntent &&
          epoch === env.target()?.epoch &&
          modalAllowed() &&
          env.canFocus() &&
          !document.querySelector("dialog[open]")
        )
          term.focus();
      });
    };
    const service = new TerminalClipboardController(
      {
        target,
        reason: () =>
          readonly
            ? "历史屏幕只读"
            : !focused
              ? "请先激活终端窗口"
              : !modalAllowed()
                ? "请先关闭当前对话框"
                : env.reason(),
        selection: () => term.getSelection(),
        bracketed: () => term.modes.bracketedPasteMode,
        paste: env.paste,
        control: env.control,
        focus,
        selectAll: () => term.selectAll(),
        clipboard: textClipboard,
        copy: copyText,
      },
      change,
    );
    controller.current = service;
    const clearComposition = () => {
      if (term.textarea) term.textarea.value = "";
      if (composing) {
        composing = false;
        // Complete xterm's public DOM composition lifecycle with an empty buffer.
        term.textarea?.dispatchEvent(
          new CompositionEvent("compositionend", { bubbles: true, data: "" }),
        );
      }
    };
    const invalidate = () => {
      env.invalidate();
      clearComposition();
      setMenu(null);
      service.invalidate();
    };
    binding.current = { term, env, readonly, invalidate, focus };
    const block = (event: Event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
    };
    const showMenu = (x?: number, y?: number) => {
      if (!env.canFocus() || !focused || !modalAllowed()) return;
      const rect = host.getBoundingClientRect();
      service.refresh();
      setMenu({ x: x ?? rect.left + 12, y: y ?? rect.top + 12 });
    };
    term.options.rightClickSelectsWord = false;
    term.options.macOptionClickForcesSelection = true;
    term.attachCustomKeyEventHandler((event) => {
      const action = resolveKeyAction(
        event,
        options.current.platform,
        options.current.preferences,
        term.hasSelection(),
      );
      if (action !== "passthrough") {
        block(event);
        if (
          event.type === "keydown" &&
          !event.repeat &&
          env.canFocus() &&
          focused &&
          modalAllowed()
        ) {
          if (action === "copy") void service.copySelection();
          if (action === "paste") void service.requestPaste();
          if (action === "clearSelection") term.clearSelection();
          if (action === "menu") showMenu();
        }
        return false;
      }
      if (!target()) {
        block(event);
        return false;
      }
      return true;
    });
    const listeners: (() => void)[] = [];
    const listen = (
      node: EventTarget,
      name: string,
      handler: (event: any) => void,
      capture = true,
    ) => {
      node.addEventListener(name, handler, capture);
      listeners.push(() => node.removeEventListener(name, handler, capture));
    };
    listen(document, "pointerdown", () => {
      focusIntent++;
    });
    listen(document, "keydown", () => {
      focusIntent++;
    });
    listen(host, "paste", (event: ClipboardEvent) => {
      block(event);
      if (env.canFocus() && focused && modalAllowed())
        void service.requestPaste({
          text: event.clipboardData?.getData("text/plain") ?? "",
        });
    });
    listen(host, "copy", (event: ClipboardEvent) => {
      block(event);
      if (env.canFocus()) void service.copySelection();
    });
    for (const name of [
      "beforeinput",
      "input",
      "compositionstart",
      "compositionupdate",
    ]) {
      listen(host, name, (event: Event) => {
        if (!target()) {
          block(event);
          clearComposition();
        } else if (name === "compositionstart") composing = true;
      });
    }
    listen(host, "compositionend", () => {
      composing = false;
      if (!target() && term.textarea) term.textarea.value = "";
    });
    const ownsRight = (event: MouseEvent) =>
      event.shiftKey || term.modes.mouseTrackingMode === "none";
    listen(host, "pointerdown", (event: PointerEvent) => {
      if (event.button === 0) selecting = true;
      if (event.button === 2 && ownsRight(event)) {
        rightAction =
          event.shiftKey ||
          !options.current.preferences.rightClickPaste ||
          readonly
            ? "menu"
            : "paste";
        // Do not prevent pointerdown: cancelling it suppresses compatibility mouse events on some WebViews.
        event.stopImmediatePropagation();
      }
    });
    listen(host, "mousedown", (event: MouseEvent) => {
      if (event.button !== 2 || !ownsRight(event)) return;
      block(event);
      rightAction ??=
        event.shiftKey ||
        !options.current.preferences.rightClickPaste ||
        readonly
          ? "menu"
          : "paste";
    });
    listen(host, "mouseup", (event: MouseEvent) => {
      if (event.button === 2 && ownsRight(event)) block(event);
    });
    listen(host, "contextmenu", (event: MouseEvent) => {
      const action = rightAction;
      rightAction = null;
      // Keyboard contextmenu has no pointer gesture and always opens the local menu.
      if (!action && event.button === 2 && !ownsRight(event)) {
        event.preventDefault();
        return;
      }
      block(event);
      if (action === "paste") void service.requestPaste();
      else showMenu(event.clientX || undefined, event.clientY || undefined);
    });
    listen(
      document,
      "pointerup",
      () => {
        if (!selecting) return;
        selecting = false;
        queueMicrotask(() => {
          if (
            disposed ||
            !env.canFocus() ||
            !options.current.preferences.copyOnSelect ||
            !term.hasSelection()
          )
            return;
          nextAutomaticCopy = term.getSelection();
          if (automaticCopyPending) return;
          automaticCopyPending = true;
          void (async () => {
            while (
              nextAutomaticCopy &&
              !disposed &&
              options.current.preferences.copyOnSelect
            ) {
              const text = nextAutomaticCopy;
              nextAutomaticCopy = undefined;
              await service.copySelection(text);
            }
          })().finally(() => {
            automaticCopyPending = false;
            nextAutomaticCopy = undefined;
          });
        });
      },
      false,
    );
    let externalModal = !modalAllowed();
    const modalObserver = new MutationObserver((records) => {
      if (
        !records.some(
          (record) =>
            record.type === "attributes" ||
            [...record.addedNodes, ...record.removedNodes].some(
              (node) =>
                node instanceof Element &&
                (node.matches("dialog") || node.querySelector("dialog")),
            ),
        )
      )
        return;
      const next = !modalAllowed();
      if (next && !externalModal) invalidate();
      if (next !== externalModal) service.refresh();
      externalModal = next;
    });
    modalObserver.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["open"],
    });
    const selection = term.onSelectionChange(() => service.refresh());
    const parsed = term.onWriteParsed(() => {
      if (service.state.preview) service.refresh();
    });
    const loseFocus = () => {
      focused = false;
      invalidate();
    };
    const gainFocus = () => {
      focused = true;
      service.refresh();
    };
    listen(
      window,
      "blur",
      (event) => {
        if (event.target === window) loseFocus();
      },
      false,
    );
    listen(
      window,
      "focus",
      (event) => {
        if (event.target === window) gainFocus();
      },
      false,
    );
    listen(document, "visibilitychange", () => {
      if (document.hidden) loseFocus();
      else if (document.hasFocus()) gainFocus();
    });
    let offNative: (() => void) | undefined;
    if (desktop)
      void getCurrentWindow()
        .onFocusChanged((event) => {
          if (disposed) return;
          if (event.payload) gainFocus();
          else loseFocus();
        })
        .then((off) => {
          if (disposed) off();
          else offNative = off;
        })
        .catch(() => {});
    service.refresh();
    return () => {
      disposed = true;
      if (focusFrame !== undefined) cancelAnimationFrame(focusFrame);
      env.invalidate();
      listeners.forEach((off) => off());
      offNative?.();
      modalObserver.disconnect();
      selection.dispose();
      parsed.dispose();
      service.dispose();
      if (controller.current === service) {
        controller.current = null;
        binding.current = null;
      }
    };
  }
  useEffect(() => {
    controller.current?.refresh();
  }, [settings]);
  const invalidate = () => binding.current?.invalidate();
  const focus = () => binding.current?.focus();
  const closeMenu = (restore: boolean) => {
    setMenu(null);
    if (restore) focus();
  };
  const ui = (
    <>
      {state.notice && (
        <div
          className="terminal-clipboard-notice"
          data-kind={state.notice.kind}
          role="status"
        >
          <span>{state.notice.text}</span>
          <button
            type="button"
            aria-label="关闭剪贴板提示"
            onClick={() => controller.current?.dismissNotice()}
          >
            ×
          </button>
        </div>
      )}
      {menu &&
        createPortal(
          <TerminalContextMenu
            x={menu.x}
            y={menu.y}
            state={state}
            labels={shortcutLabels(platform, preferences)}
            readonly={binding.current?.readonly ?? true}
            onClose={closeMenu}
            onAction={(action) => {
              setMenu(null);
              const c = controller.current;
              if (!c) return;
              if (action === "copy") void c.copySelection().then(focus);
              if (action === "paste" || action === "preview")
                void c.requestPaste({ forcePreview: action === "preview" });
              if (action === "selectAll") {
                c.selectAllBuffer();
                focus();
              }
              if (action === "ctrlC" || action === "ctrlV")
                void c.sendControl(action === "ctrlC" ? "C" : "V");
              if (action === "settings") {
                invalidate();
                setSettingsOpen(true);
              }
            }}
          />,
          document.body,
        )}
      {state.preview &&
        createPortal(
          <TerminalPasteDialog
            key={state.preview.id}
            owner={owner}
            preview={state.preview}
            bracketed={binding.current?.term.modes.bracketedPasteMode ?? false}
            onCancel={() => controller.current?.cancel()}
            onConfirm={(text) =>
              void controller.current?.confirm(state.preview!.id, text)
            }
          />,
          document.body,
        )}
      {settingsOpen &&
        createPortal(
          <SystemSettingsDialog
            section="terminal"
            onClose={() => {
              modalOpen.current = false;
              setSettingsOpen(false);
              queueMicrotask(focus);
            }}
          />,
          document.body,
        )}
    </>
  );
  const attachReadonly = (term: Terminal) =>
    attach(
      term,
      {
        target: () => null,
        reason: () => "历史屏幕只读",
        invalidate: () => {},
        canFocus: () => !!term.element?.isConnected,
        paste: async () => {
          throw new Error("历史屏幕只读");
        },
        control: async () => {
          throw new Error("历史屏幕只读");
        },
      },
      true,
    );
  return { attach, attachReadonly, controller, state, ui, invalidate, focus };
}
