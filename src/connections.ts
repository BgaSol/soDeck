import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { escape as e } from "./ui";
import type { Context } from "./workspaces";

type Question = { kind: "credentials"; id: string; nodeIndex: number; name: string; endpoint: string; authentication: string; username: string; privateKeyPath: string | null }
  | { kind: "hostKey"; id: string; nodeIndex: number; name: string; endpoint: string; algorithm: string; fingerprint: string; previousFingerprint: string | null }
  | { kind: "targetChange"; id: string; previousEndpoint: string; endpoint: string };
export interface ConnectionView {
  attemptId: string; serverId: string; windowId: string; state: string; stage: string; nodeIndex: number; nodeCount: number; endpoint: string;
  question: Question | null; context: Context | null; error: { code: string; stage: string; message: string } | null; sentBytes: string; receivedBytes: string;
}
const stages: Record<string, string> = { preparing: "准备连接资料", credentials: "补充连接凭据", dns: "解析第一站地址", tcp: "访问第一站", sshHandshake: "SSH 握手与主机检查", authentication: "SSH 认证", privateKey: "读取私钥", sshTunnel: "建立 SSH 隧道", socks5: "通过 SOCKS5 访问下一站", connected: "已连接" };
export function connectionSummary(v: ConnectionView) {
  return v.state === "connecting" ? `第 ${v.nodeIndex + 1} / ${v.nodeCount} 站 · ${v.question ? "等待确认" : stages[v.stage] ?? v.stage}` : v.state === "connected" ? "已连接" : v.error?.message ?? (v.state === "cancelled" ? "已取消" : "已断开");
}
export function connectionCard(id: string, v?: ConnectionView) {
  const busy = v?.state === "connecting", online = v?.state === "connected";
  return `<section class="connection-card" aria-label="服务器连接"><div><strong>${online ? "远程会话已就绪" : busy ? "正在连接" : "远程连接"}</strong><p role="status">${v ? e(connectionSummary(v)) : "通过保存的访问路径连接 SSH 服务器"}</p></div><div class="actions">${busy ? `<button data-connection-cancel="${e(v.attemptId)}">取消连接</button>${v.question ? `<button data-connection-prompt="${e(v.attemptId)}" class="primary">继续确认</button>` : ""}` : `<button data-connect-server="${e(id)}" class="primary">${online ? "前往工作台" : v ? "重新连接" : "连接服务器"}</button>`}</div></section>`;
}
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export class ConnectionsUi {
  views: ConnectionView[] = [];
  private refreshing?: Promise<void>;
  private again = false;
  private dialog?: { node: HTMLDialogElement; id: string; attempt: string; submitted: boolean };
  private dismissed = new Set<string>();
  constructor(private windowId: string, private update: (views: ConnectionView[]) => void, private notify: (text: string) => void, private call: Call = invoke) {
    document.addEventListener("click", event => {
      const b = (event.target as Element).closest<HTMLElement>("[data-connect-server],[data-connection-cancel],[data-connection-prompt],[data-disconnect-context],[data-reconnect-server]");
      if (b) void this.action(b).catch(error => notify(String(error)));
    });
  }
  async refresh(): Promise<void> {
    if (this.refreshing) { this.again = true; return this.refreshing; }
    this.refreshing = (async () => { do { this.again = false; this.apply(await this.call<ConnectionView[]>("connection", { command: { op: "snapshot" } })); } while (this.again); })().finally(() => { this.refreshing = undefined; });
    return this.refreshing;
  }
  private async send(command: Record<string, unknown>) {
    // Query again after the command so a racing progress event cannot leave stale state.
    await this.call<ConnectionView[]>("connection", { command }); await this.refresh();
  }
  private apply(views: ConnectionView[]) {
    this.dismissed = new Set([...this.dismissed].filter(id => views.some(v => v.question?.id === id)));
    this.views = views; this.update(views);
    if (this.dialog && !views.some(v => v.attemptId === this.dialog!.attempt && v.question?.id === this.dialog!.id)) this.closePrompt();
    if (!this.dialog) {
      const v = views.find(v => v.windowId === this.windowId && v.state === "connecting" && v.question && !this.dismissed.has(v.question.id));
      if (v) this.prompt(v);
    }
  }
  private async action(b: HTMLElement) {
    const server = b.dataset.connectServer ?? b.dataset.reconnectServer;
    if (server) {
      const v = this.views.find(v => v.serverId === server);
      if (v?.state === "connected" && v.context) await this.send({ op: "focus", context: v.context });
      else await this.send({ op: "start", serverId: server });
    } else if (b.dataset.connectionCancel) await this.send({ op: "cancel", attemptId: b.dataset.connectionCancel });
    else if (b.dataset.connectionPrompt) { const v = this.views.find(v => v.attemptId === b.dataset.connectionPrompt); if (v) this.prompt(v); }
    else if (b.dataset.disconnectContext) await this.send({ op: "disconnect", context: JSON.parse(b.dataset.disconnectContext) });
  }
  private closePrompt() { this.dialog?.node.close(); this.dialog?.node.remove(); this.dialog = undefined; }
  private prompt(v: ConnectionView) {
    const q = v.question; if (!q || this.dialog?.id === q.id) return;
    this.closePrompt();
    const node = document.createElement("dialog"); node.className = "catalog-dialog connection-dialog";
    const title = q.kind === "credentials" ? "补充连接凭据" : q.kind === "targetChange" ? "重连目标已变更" : q.previousFingerprint ? "SSH 主机密钥已变化" : "确认 SSH 主机身份";
    node.setAttribute("aria-label", title);
    const content = q.kind === "credentials" ? `<p>第 ${q.nodeIndex + 1} 站 · ${e(q.name)} · ${e(q.endpoint)}</p><p class="hint">账号 ${e(q.username)} · 此处输入仅用于本次连接，记住凭据请编辑服务器资料。</p><form id="connection-auth">${q.authentication === "privateKey" ? `<label>私钥文件<div class="connection-key-path"><input name="path" value="${e(q.privateKeyPath ?? "")}" required><button type="button" data-key-picker>选择文件</button></div></label>` : ""}<label>${q.authentication === "privateKey" ? "私钥口令" : "密码"}<input name="secret" type="password" autocomplete="new-password" ${q.authentication === "privateKey" ? "" : "required"}></label></form>`
      : q.kind === "targetChange" ? `<p>服务器的地址、账号、认证方式或访问路径已修改。</p><dl><dt>原连接</dt><dd>${e(q.previousEndpoint)}</dd><dt>当前目标</dt><dd>${e(q.endpoint)}</dd></dl><p>按新资料重连后，旧会话资源仍会失效。</p>`
      : `<p>第 ${q.nodeIndex + 1} 站 · ${e(q.name)} · ${e(q.endpoint)}</p>${q.previousFingerprint ? `<p class="error-box">已记录的主机密钥与本次不同，请核对服务器身份后再更新。</p><label>原指纹<code>${e(q.previousFingerprint)}</code></label>` : "<p>请核对该服务器的主机指纹。信任后才会提交此节点的认证凭据。</p>"}<label>${e(q.algorithm)}<code>${e(q.fingerprint)}</code></label>`;
    node.innerHTML = `<header class="catalog-dialog-heading"><h2>${title}</h2></header><section class="connection-prompt-body">${content}<p class="connection-prompt-error error-box" role="alert" hidden></p></section><footer class="catalog-dialog-footer"><button data-prompt-cancel>取消连接</button><button class="primary" data-prompt-accept>${q.kind === "credentials" ? "继续连接" : q.kind === "targetChange" ? "按新资料重连" : q.previousFingerprint ? "更新指纹并继续" : "信任并继续"}</button></footer>`;
    const current = { node, id: q.id, attempt: v.attemptId, submitted: false }; this.dialog = current;
    const submit = async (cancel: boolean) => {
      if (current.submitted || this.dialog !== current) return;
      if (!cancel && q.kind === "credentials" && !node.querySelector<HTMLFormElement>("form")!.reportValidity()) return;
      current.submitted = true; node.querySelectorAll<HTMLButtonElement>("button").forEach(b => b.disabled = true);
      this.dismissed.add(q.id);
      try {
        if (cancel) await this.send({ op: "cancel", attemptId: v.attemptId });
        else {
          const secret = node.querySelector<HTMLInputElement>('[name="secret"]')?.value ?? "";
          const path = node.querySelector<HTMLInputElement>('[name="path"]')?.value ?? null;
          await this.send({ op: "answer", attemptId: v.attemptId, questionId: q.id, answer: q.kind === "credentials" ? { kind: "credentials", secret, privateKeyPath: path } : { kind: q.kind, accept: true } });
        }
        if (this.dialog === current) this.closePrompt();
        await this.refresh();
      } catch (error) {
        this.dismissed.delete(q.id);
        if (this.dialog !== current) return;
        current.submitted = false; node.querySelectorAll<HTMLButtonElement>("button").forEach(b => b.disabled = false);
        const target = node.querySelector<HTMLElement>(".connection-prompt-error")!; target.hidden = false; target.textContent = String(error);
      } finally { const input = node.querySelector<HTMLInputElement>('[name="secret"]'); if (input) input.value = ""; }
    };
    node.addEventListener("cancel", event => { event.preventDefault(); void submit(true); });
    node.querySelector("[data-prompt-cancel]")!.addEventListener("click", () => void submit(true));
    node.querySelector("[data-prompt-accept]")!.addEventListener("click", () => void submit(false));
    node.querySelector("form")?.addEventListener("submit", event => { event.preventDefault(); void submit(false); });
    node.querySelector("[data-key-picker]")?.addEventListener("click", () => { void open({ multiple: false }).then(path => { if (typeof path === "string" && this.dialog === current) node.querySelector<HTMLInputElement>('[name="path"]')!.value = path; }).catch(error => this.notify(String(error))); });
    document.body.append(node); node.showModal(); node.querySelector<HTMLElement>("input,button")?.focus();
  }
}
