import { useEffect, useState } from "react";
import { Folder, File, ArrowUp } from "lucide-react";
import { api } from "../api";
import { errorOf, type Directory } from "../types";
export function DockerPathPicker({
  contextId,
  initialPath,
  onPick,
  onClose,
}: {
  contextId: string;
  initialPath: string;
  onPick: (path: string) => void;
  onClose: () => void;
}) {
  const [path, setPath] = useState(initialPath || "/"),
    [input, setInput] = useState(initialPath || "/"),
    [directory, setDirectory] = useState<Directory | null>(null),
    [error, setError] = useState("");
  useEffect(() => {
    let alive = true;
    setDirectory(null);
    setError("");
    void api.directory(contextId, path).then(
      (d) => {
        if (alive) {
          setDirectory(d);
          setInput(d.path);
        }
      },
      (e) => {
        if (alive) setError(errorOf(e).message);
      },
    );
    return () => {
      alive = false;
    };
  }, [contextId, path]);
  return (
    <section className="docker-path-picker" aria-label="选择远端配置文件">
      <div className="docker-inline-tools">
        <button
          type="button"
          className="icon-button"
          aria-label="上级目录"
          onClick={() => setPath(path.replace(/\/[^/]+\/?$/, "") || "/")}
        >
          <ArrowUp size={14} />
        </button>
        <input
          aria-label="远端目录"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              setPath(input);
            }
          }}
        />
        <button type="button" onClick={() => setPath(input)}>
          打开
        </button>
      </div>
      {error && <p className="resource-error">{error}</p>}
      <div className="docker-path-list">
        {directory?.entries.map((entry) => (
          <button
            type="button"
            key={entry.path}
            onClick={() =>
              entry.isDir ? setPath(entry.path) : onPick(entry.path)
            }
          >
            {entry.isDir ? <Folder size={13} /> : <File size={13} />}
            <span>{entry.name}</span>
          </button>
        ))}
        {!directory && !error && <p className="resource-help">正在读取…</p>}
      </div>
      <button type="button" className="resource-link" onClick={onClose}>
        收起文件选择
      </button>
    </section>
  );
}
