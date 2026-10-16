import { DialogHeader } from "./DialogHeader";
import { useEffect, useRef, useState } from "react";
import { ArrowRightLeft, RefreshCw } from "lucide-react";
import { transferApi } from "./transferApi";
import type { CopyTarget, TransferDraft } from "./transferApi";
import { errorOf } from "./types";

export function CopyTargetDialog({
  sessionId,
  paths,
  onClose,
  onPrepared,
}: {
  sessionId: string;
  paths: string[];
  onClose: () => void;
  onPrepared: (draft: TransferDraft) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [targets, setTargets] = useState<CopyTarget[]>([]);
  const [selected, setSelected] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  useEffect(() => {
    let live = true;
    setLoading(true);
    setError("");
    void transferApi
      .copyTargets(sessionId)
      .then((next) => {
        if (live) {
          setTargets(next);
          setSelected(next[0]?.token ?? "");
        }
      })
      .catch((e) => {
        if (live) setError(errorOf(e).message);
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [sessionId, revision]);
  async function prepare() {
    if (!selected || busy || loading) return;
    setBusy(true);
    setError("");
    try {
      onPrepared(await transferApi.prepareCopy(sessionId, paths, selected));
    } catch (e) {
      setError(errorOf(e).message);
      setBusy(false);
    }
  }
  return (
    <dialog
      ref={dialog}
      className="file-operation-dialog"
      aria-labelledby="copy-target-title"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void prepare();
        }}
      >
        <DialogHeader
          id="copy-target-title"
          title={
            <>
              <ArrowRightLeft size={16} aria-hidden="true" />
              复制到另一台服务器
            </>
          }
          closeLabel="关闭目标选择"
          onClose={onClose}
          disabled={busy}
        />
        <div className="file-operation-fields">
          <p className="muted">
            选择目标会话中已打开的目录。下一步确认连接地址和同名处理方式。
          </p>
          <div className="direct-endpoint-heading">
            <strong>{paths.length} 个项目</strong>
            <button
              type="button"
              className="quiet"
              disabled={busy || loading}
              onClick={() => setRevision((value) => value + 1)}
            >
              <RefreshCw size={14} />
              刷新目标
            </button>
          </div>
          {loading ? (
            <p role="status">正在读取目标目录…</p>
          ) : targets.length ? (
            <fieldset className="copy-target-list" disabled={busy}>
              <legend className="sr-only">目标服务器与目录</legend>
              {targets.map((target) => (
                <label
                  key={target.token}
                  className={selected === target.token ? "selected" : ""}
                >
                  <input
                    type="radio"
                    name="copy-target"
                    value={target.token}
                    checked={selected === target.token}
                    onChange={() => setSelected(target.token)}
                  />
                  <span>
                    <strong>{target.serverName}</strong>
                    <span title={target.path}>{target.path}</span>
                  </span>
                </label>
              ))}
            </fieldset>
          ) : (
            <p className="copy-target-empty">
              请在另一个会话或窗口连接目标服务器，并在文件面板打开要接收文件的目录，然后刷新目标。
            </p>
          )}
          {error && (
            <p className="panel-error" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="dialog-actions">
          <button type="button" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="primary" disabled={!selected || loading || busy}>
            {busy ? "正在准备…" : "下一步"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
