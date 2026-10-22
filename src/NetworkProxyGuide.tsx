import { useEffect, useState } from "react";
import { Check, Copy } from "lucide-react";
import { curlExamples, proxyAddresses } from "./networkProxyGuideUtils";

export function NetworkProxyGuide({
  address,
  ready,
  onCopy,
}: {
  address: string | null;
  ready: boolean;
  onCopy?: (value: string) => Promise<void>;
}) {
  const [target, setTarget] = useState("http://service.internal:8080/");
  const [shell, setShell] = useState<"windows" | "posix">(
    typeof navigator !== "undefined" && /Windows/i.test(navigator.platform)
      ? "windows"
      : "posix",
  );
  const [copied, setCopied] = useState("");
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(""), 1600);
    return () => window.clearTimeout(timer);
  }, [copied]);
  async function copy(value: string, label: string) {
    try {
      if (onCopy) await onCopy(value);
      else await navigator.clipboard.writeText(value);
      setCopied(label);
    } catch {
      // The parent detail surface reports clipboard errors when it owns copy.
    }
  }
  const resolved = address ? proxyAddresses(address) : null;
  const examples = curlExamples(resolved?.socks5h || "socks5h://127.0.0.1:1080", target);
  return (
    <section className="proxy-guide" aria-labelledby="proxy-guide-title">
      <div className="forward-section-head">
        <h3 id="proxy-guide-title">如何使用</h3>
        <span>HTTP · SOCKS5</span>
      </div>
      {!resolved || !ready ? (
        <p className="field-help">
          SSH 路径启动后显示代理地址；它只监听本机回环地址。
        </p>
      ) : (
        <>
          <div className="proxy-addresses">
            {[
              ["HTTP 代理", resolved.http],
              ["SOCKS5 代理", resolved.socks5],
              ["SOCKS5 · 现场 DNS", resolved.socks5h],
            ].map(([label, value]) => (
              <div className="proxy-address-row" key={label}>
                <span>{label}</span>
                <code>{value}</code>
                <button
                  className="icon-button"
                  aria-label={`复制${label}`}
                  title={`复制${label}`}
                  onClick={() => void copy(value, label)}
                >
                  {copied === label ? <Check size={14} /> : <Copy size={14} />}
                </button>
              </div>
            ))}
          </div>
          <p className="field-help">
            使用 <code>socks5h://</code> 时，主机名由最终 SSH 节点解析；HTTP 代理适合浏览器和普通 curl 请求。
          </p>
          <div className="proxy-command-head">
            <strong>curl 示例</strong>
            <div className="proxy-shell-switch" role="group" aria-label="命令行平台">
              <button
                type="button"
                className={shell === "windows" ? "selected" : ""}
                aria-pressed={shell === "windows"}
                onClick={() => setShell("windows")}
              >
                Windows PowerShell
              </button>
              <button
                type="button"
                className={shell === "posix" ? "selected" : ""}
                aria-pressed={shell === "posix"}
                onClick={() => setShell("posix")}
              >
                Linux / macOS
              </button>
            </div>
          </div>
          <label className="proxy-target-example">
            <span>示例目标</span>
            <input
              value={target}
              onChange={(event) => setTarget(event.target.value)}
              spellCheck={false}
              aria-label="curl 示例目标"
            />
          </label>
          <div className="proxy-command" role="group" aria-label="curl 命令">
            <code>{examples[shell]}</code>
            <button
              className="icon-button"
              aria-label="复制 curl 命令"
              title="复制 curl 命令"
              onClick={() => void copy(examples[shell], "curl 命令")}
            >
              {copied === "curl 命令" ? <Check size={14} /> : <Copy size={14} />}
            </button>
          </div>
          <p className="field-help">
            示例只复制命令，不会执行；<code>--noproxy ""</code> 仅让这一次 curl 不受本机绕过规则影响。
          </p>
        </>
      )}
    </section>
  );
}
