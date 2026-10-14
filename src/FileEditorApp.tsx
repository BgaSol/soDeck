import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { basicSetup } from "codemirror";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { openSearchPanel } from "@codemirror/search";
import { indentWithTab, redo, undo } from "@codemirror/commands";
import {
  detectLanguage,
  languages,
  languageName,
  loadLanguage,
  type LanguageMode,
} from "./editorLanguages";
import { FileText, RefreshCw, Save, Search, Undo2, Redo2 } from "lucide-react";
import { desktop } from "./api";
import { fileApi } from "./fileApi";
import type { EditorDocument } from "./fileApi";
import { errorOf, formatSize } from "./types";
import "./files.css";

type Prompt = "close" | "reload" | "conflict" | null;

export function FileEditorApp() {
  const [document, setDocument] = useState<EditorDocument | null>(null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [prompt, setPrompt] = useState<Prompt>(null);
  const [cursor, setCursor] = useState({ line: 1, column: 1 });
  const [size, setSize] = useState(0);
  const [languageMode, setLanguageMode] = useState<LanguageMode>("auto");
  const [firstLine, setFirstLine] = useState("");
  const [languageError, setLanguageError] = useState("");
  const language = useRef(new Compartment());
  const loadedLanguage = useRef<Extension>([]);
  const detectedLanguage = detectLanguage(document?.path || "", firstLine);
  const chosenLanguage =
    languageMode === "auto" ? detectedLanguage : languageMode;
  const parent = useRef<HTMLDivElement>(null);
  const editor = useRef<EditorView | null>(null);
  const editing = useRef(new Compartment());
  const dialog = useRef<HTMLDialogElement>(null);
  const documentRef = useRef<EditorDocument | null>(null);
  const saved = useRef("");
  const text = useRef("");
  const dirtyRef = useRef(false);
  const busyRef = useRef(false);
  const closeAllowed = useRef(false);
  const closeAfterSave = useRef(false);
  const saveRef = useRef<(force?: boolean, close?: boolean) => Promise<void>>(
    async () => {},
  );
  const loadRef = useRef<() => Promise<void>>(async () => {});

  const setChanged = useCallback((next: string) => {
    text.current = next;
    dirtyRef.current = next !== saved.current;
    setDirty(dirtyRef.current);
    setSize(new TextEncoder().encode(next).length);
    setNotice("");
    setFirstLine(next.slice(0, 512).split(/[\r\n]/, 1)[0]);
  }, []);

  const makeState = useCallback(
    (value: string) =>
      EditorState.create({
        doc: value,
        extensions: [
          basicSetup,
          editing.current.of([
            EditorState.readOnly.of(false),
            EditorView.editable.of(true),
          ]),
          EditorState.lineSeparator.of(value.includes("\r\n") ? "\r\n" : "\n"),
          keymap.of([
            {
              key: "Mod-s",
              run: () => {
                void saveRef.current();
                return true;
              },
            },
            indentWithTab,
          ]),
          language.current.of(loadedLanguage.current),
          syntaxHighlighting(
            HighlightStyle.define([
              { tag: tags.keyword, color: "var(--editor-keyword)" },
              {
                tag: [tags.string, tags.regexp],
                color: "var(--editor-string)",
              },
              {
                tag: [tags.number, tags.bool, tags.null],
                color: "var(--editor-number)",
              },
              { tag: tags.comment, color: "var(--muted)", fontStyle: "italic" },
              {
                tag: [tags.propertyName, tags.function(tags.variableName)],
                color: "var(--editor-property)",
              },
              {
                tag: [tags.typeName, tags.className],
                color: "var(--editor-type)",
              },
              {
                tag: [tags.variableName, tags.attributeName],
                color: "var(--editor-property)",
              },
              {
                tag: [tags.operator, tags.meta, tags.tagName],
                color: "var(--editor-keyword)",
              },
              {
                tag: tags.heading,
                color: "var(--editor-property)",
                fontWeight: "bold",
              },
              {
                tag: tags.link,
                color: "var(--editor-string)",
                textDecoration: "underline",
              },
              { tag: tags.strong, fontWeight: "bold" },
              { tag: tags.emphasis, fontStyle: "italic" },
              {
                tag: tags.invalid,
                textDecoration: "underline wavy var(--error)",
              },
            ]),
          ),
          EditorView.contentAttributes.of({
            "aria-label": "远程文件内容",
            spellcheck: "false",
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) setChanged(update.state.sliceDoc());
            if (update.selectionSet || update.docChanged) {
              const head = update.state.selection.main.head;
              const line = update.state.doc.lineAt(head);
              setCursor({ line: line.number, column: head - line.from + 1 });
            }
          }),
          EditorView.theme({
            "&": {
              height: "100%",
              backgroundColor: "var(--surface)",
              color: "var(--text)",
            },
            ".cm-scroller": {
              overflow: "auto",
              fontFamily: "var(--mono)",
              fontSize: "13px",
              lineHeight: "1.65",
            },
            ".cm-content": { caretColor: "var(--text)", padding: "12px 0" },
            ".cm-gutters": {
              backgroundColor: "var(--canvas)",
              color: "var(--muted)",
              borderRight: "1px solid var(--line)",
            },
            ".cm-activeLine, .cm-activeLineGutter": {
              backgroundColor: "var(--hover)",
            },
            "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection":
              { backgroundColor: "var(--selected)" },
            ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text)" },
            ".cm-panels": {
              backgroundColor: "var(--canvas)",
              color: "var(--text)",
              borderColor: "var(--line)",
            },
            ".cm-searchMatch": {
              backgroundColor: "#d6b64055",
              outline: "1px solid #b8972e",
            },
            ".cm-searchMatch-selected": { backgroundColor: "#d6b640aa" },
          }),
        ],
      }),
    [setChanged],
  );

  const load = useCallback(async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    editor.current?.dispatch({
      effects: editing.current.reconfigure([
        EditorState.readOnly.of(true),
        EditorView.editable.of(false),
      ]),
    });
    try {
      const next = await fileApi.loadEditor();
      documentRef.current = next;
      saved.current = next.text;
      text.current = next.text;
      setFirstLine(next.text.slice(0, 512).split(/[\r\n]/, 1)[0]);
      dirtyRef.current = false;
      setDirty(false);
      setDocument(next);
      setSize(next.size);
      setCursor({ line: 1, column: 1 });
      if (editor.current) editor.current.setState(makeState(next.text));
      setPrompt(null);
      closeAfterSave.current = false;
    } catch (error) {
      setError(errorOf(error).message);
    } finally {
      editor.current?.dispatch({
        effects: editing.current.reconfigure([
          EditorState.readOnly.of(false),
          EditorView.editable.of(true),
        ]),
      });
      busyRef.current = false;
      setBusy(false);
    }
  }, [makeState]);
  loadRef.current = load;

  async function closeWindow() {
    closeAllowed.current = true;
    if (desktop) await getCurrentWindow().close();
  }

  async function save(force = false, close = false) {
    const current = documentRef.current;
    if (!current || busyRef.current) return;
    if (!dirtyRef.current && !force) {
      if (close) await closeWindow();
      return;
    }
    if (new TextEncoder().encode(text.current).length > 1024 * 1024) {
      setError("文件超过 1 MB，无法在线保存。请缩小内容后再保存。");
      setPrompt(null);
      return;
    }
    closeAfterSave.current = close || closeAfterSave.current;
    const value = text.current;
    busyRef.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const next = await fileApi.saveEditor(value, current.version, force);
      documentRef.current = next;
      saved.current = value;
      dirtyRef.current = text.current !== value;
      setDirty(dirtyRef.current);
      setDocument(next);
      setPrompt(null);
      setNotice("已保存到服务器");
      const shouldClose = closeAfterSave.current && !dirtyRef.current;
      closeAfterSave.current = false;
      if (shouldClose) await closeWindow();
    } catch (error) {
      const detail = errorOf(error);
      if (detail.kind === "conflict") setPrompt("conflict");
      else {
        setError(detail.message);
        setPrompt(null);
        closeAfterSave.current = false;
      }
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }
  saveRef.current = save;

  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    if (!document || !parent.current || editor.current) return;
    editor.current = new EditorView({
      state: makeState(text.current),
      parent: parent.current,
    });
    editor.current.focus();
  }, [document, makeState]);
  useEffect(() => {
    let disposed = false;
    setLanguageError("");
    void loadLanguage(chosenLanguage)
      .then((extension) => {
        if (disposed) return;
        loadedLanguage.current = extension;
        editor.current?.dispatch({
          effects: language.current.reconfigure(extension),
        });
      })
      .catch(() => {
        if (disposed) return;
        loadedLanguage.current = [];
        editor.current?.dispatch({ effects: language.current.reconfigure([]) });
        setLanguageError(
          "语法高亮加载失败，仍可编辑和保存；可重新选择语言重试。",
        );
      });
    return () => {
      disposed = true;
    };
  }, [chosenLanguage]);
  useEffect(
    () => () => {
      editor.current?.destroy();
      editor.current = null;
    },
    [],
  );
  useEffect(() => {
    if (prompt) dialog.current?.showModal();
    else dialog.current?.close();
  }, [prompt]);
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested((event) => {
        if (closeAllowed.current) return;
        if (dirtyRef.current || busyRef.current) {
          event.preventDefault();
          setPrompt("close");
        }
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch((error) => setError(errorOf(error).message));
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  useEffect(() => {
    if (!desktop || !document) return;
    void getCurrentWindow()
      .setTitle(
        (dirty ? "● " : "") +
          document.path.split("/").pop() +
          " — " +
          document.serverName,
      )
      .catch(() => {});
  }, [dirty, document]);
  useEffect(() => {
    if (!desktop || !document) return;
    void fileApi
      .setEditorDirty(dirty)
      .catch((error) => setError(errorOf(error).message));
  }, [dirty, document]);

  function cancelPrompt() {
    closeAfterSave.current = false;
    setPrompt(null);
    editor.current?.focus();
  }

  return (
    <div className="remote-editor-app">
      <header className="remote-editor-header">
        <div className="remote-editor-title">
          <FileText size={19} />
          <div>
            <h1>
              {document?.path.split("/").pop() || "远程文件编辑"}
              {dirty && (
                <span className="editor-dirty" aria-label="未保存">
                  ●
                </span>
              )}
            </h1>
            <p>
              {document
                ? document.serverName + " · " + document.path
                : "正在读取远程文件…"}
            </p>
          </div>
        </div>
        <button
          className="primary"
          disabled={!document || busy || !dirty}
          onClick={() => void save()}
        >
          <Save size={15} />
          {busy ? "正在处理…" : "保存"}
        </button>
      </header>
      <div className="remote-editor-tools">
        <button
          className="quiet"
          disabled={!document || busy}
          onClick={() =>
            dirtyRef.current ? setPrompt("reload") : void loadRef.current()
          }
        >
          <RefreshCw size={14} />
          重新加载
        </button>
        <button
          className="quiet"
          disabled={!document}
          onClick={() => editor.current && openSearchPanel(editor.current)}
        >
          <Search size={14} />
          查找
        </button>
        <button
          className="icon-button"
          aria-label="撤销"
          disabled={!document}
          onClick={() => editor.current && undo(editor.current)}
        >
          <Undo2 size={15} />
        </button>
        <button
          className="icon-button"
          aria-label="重做"
          disabled={!document}
          onClick={() => editor.current && redo(editor.current)}
        >
          <Redo2 size={15} />
        </button>
        <span className="editor-shortcut">Ctrl / ⌘ S 保存</span>
      </div>
      {error && (
        <div className="editor-error" role="alert">
          {error}
          {!document && <button onClick={() => void load()}>重试读取</button>}
        </div>
      )}
      <div ref={parent} className="remote-code-editor" />
      {languageError && (
        <div className="editor-error" role="alert">
          {languageError}
        </div>
      )}
      <footer className="remote-editor-status">
        <span role="status">
          {notice ||
            (dirty
              ? "未保存"
              : document
                ? "已与远端同步"
                : busy
                  ? "正在读取…"
                  : "读取失败")}
        </span>
        <select
          className="editor-language"
          aria-label="文件语言"
          value={languageMode}
          onChange={(event) =>
            setLanguageMode(event.target.value as LanguageMode)
          }
        >
          <option value="auto">自动：{languageName(detectedLanguage)}</option>
          {languages.map(([id, name]) => (
            <option key={id} value={id}>
              {name}
            </option>
          ))}
        </select>
        <span>
          {cursor.line}:{cursor.column} · UTF-8 · {formatSize(size)} / 1 MB
        </span>
      </footer>
      <dialog
        ref={dialog}
        className="file-confirm-dialog"
        onCancel={(event) => {
          event.preventDefault();
          cancelPrompt();
        }}
      >
        <h2>
          {prompt === "conflict"
            ? "远端文件已发生变化"
            : prompt === "close"
              ? "保存修改后关闭？"
              : "重新加载远端文件？"}
        </h2>
        <p>
          {prompt === "conflict"
            ? "其他操作修改了远端文件。重新加载会放弃当前修改；覆盖远端会保存当前编辑内容。"
            : prompt === "close"
              ? "此文件有尚未保存到服务器的修改。"
              : "重新加载会放弃当前未保存的修改。"}
        </p>
        <div className="dialog-actions">
          <button onClick={cancelPrompt}>继续编辑</button>
          {prompt === "close" && (
            <>
              <button
                className="danger"
                disabled={busy}
                onClick={() => void closeWindow()}
              >
                放弃修改
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() => void save(false, true)}
              >
                保存并关闭
              </button>
            </>
          )}
          {prompt === "reload" && (
            <button
              className="primary"
              disabled={busy}
              onClick={() => void loadRef.current()}
            >
              重新加载远端
            </button>
          )}
          {prompt === "conflict" && (
            <>
              <button disabled={busy} onClick={() => void loadRef.current()}>
                重新加载远端
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() => void save(true)}
              >
                覆盖远端
              </button>
            </>
          )}
        </div>
      </dialog>
    </div>
  );
}
