import { useEffect, useRef, useState } from "react";
import { DialogHeader } from "../DialogHeader";
import {
  analyzePaste,
  finalPasteSize,
  INPUT_LIMIT,
  removeBlockedControls,
  removeTrailingLineBreaks,
} from "./policy";
import type { ClipboardViewState } from "./types";
export function TerminalPasteDialog({
  preview,
  owner,
  bracketed,
  onCancel,
  onConfirm,
}: {
  preview: NonNullable<ClipboardViewState["preview"]>;
  owner: string;
  bracketed: boolean;
  onCancel(): void;
  onConfirm(text: string): void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const [text, setText] = useState(preview.text);
  const [showWhitespace, setShowWhitespace] = useState(
    () => analyzePaste(preview.text).controlCount > 0,
  );
  const analysis = analyzePaste(text);
  const tooLarge = finalPasteSize(text, bracketed) > INPUT_LIMIT;
  useEffect(() => {
    dialog.current?.showModal();
    input.current?.focus();
    return () => dialog.current?.close();
  }, []);
  const source = preview.source;
  return (
    <dialog
      ref={dialog}
      className="terminal-paste-dialog"
      data-clipboard-owner={owner}
      aria-labelledby={owner + "-paste-title"}
      onCancel={(event) => {
        event.preventDefault();
        onCancel();
      }}
    >
      <DialogHeader
        id={owner + "-paste-title"}
        title="粘贴到终端"
        closeLabel="取消粘贴"
        onClose={onCancel}
      />
      <p className="terminal-paste-source">
        {source.serverName}
        {source.containerName ? " / " + source.containerName : ""}
        <br />
        <span>
          {source.username}@{source.host}:{source.port}
        </span>
      </p>
      {analysis.hasLineBreak && (
        <p className="terminal-paste-warning">
          内容包含换行，远端程序可能立即处理其中的命令。
        </p>
      )}
      <textarea
        ref={input}
        aria-label="粘贴内容"
        spellCheck={false}
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      <div className="terminal-paste-details">
        <span>
          {analysis.lineCount} 行 · {analysis.utf8Bytes.toLocaleString()} 字节
        </span>
        <label>
          <input
            type="checkbox"
            checked={showWhitespace}
            onChange={(event) => setShowWhitespace(event.target.checked)}
          />
          显示空白和控制字符
        </label>
      </div>
      {showWhitespace && (
        <pre
          className="terminal-whitespace-preview"
          aria-label="空白和控制字符预览"
        >
          {text
            .slice(0, 8192)
            .replace(/ /g, "·")
            .replace(/\t/g, "→")
            .replace(/\r/g, "␍")
            .replace(/\n/g, "␊\n")
            .replace(
              /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g,
              (char) =>
                "[U+" +
                char.charCodeAt(0).toString(16).toUpperCase().padStart(4, "0") +
                "]",
            )}
          {text.length > 8192
            ? "\n…符号预览仅显示前 8192 字符；上方编辑框保留全部内容。"
            : ""}
        </pre>
      )}
      <div className="terminal-paste-adjustments">
        {analysis.hasTrailingLineBreak && (
          <button
            type="button"
            onClick={() => setText(removeTrailingLineBreaks(text))}
          >
            移除末尾换行
          </button>
        )}
        {analysis.controlCount > 0 && (
          <>
            <span role="alert">
              包含 {analysis.controlCount} 个控制字符，处理后才能粘贴。
            </span>
            <button
              type="button"
              onClick={() => setText(removeBlockedControls(text))}
            >
              移除控制字符
            </button>
          </>
        )}
      </div>
      {tooLarge && (
        <p className="panel-error" role="alert">
          最终输入超过 1 MiB，请使用文件上传或远程编辑器。
        </p>
      )}
      <footer className="dialog-actions">
        <button type="button" onClick={onCancel}>
          取消
        </button>
        <button
          type="button"
          className="primary"
          disabled={!text || !!analysis.controlCount || tooLarge}
          onClick={() => onConfirm(text)}
        >
          粘贴
        </button>
      </footer>
    </dialog>
  );
}
