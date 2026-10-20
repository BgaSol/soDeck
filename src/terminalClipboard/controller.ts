import { analyzePaste, INPUT_LIMIT, finalPasteSize } from "./policy";
import {
  emptyClipboardState,
  type ClipboardViewState,
  type PasteRequest,
  type PasteTarget,
} from "./types";
import type { TextClipboard } from "./clipboardService";
export interface ClipboardEnvironment {
  target(): PasteTarget | null;
  reason(): string;
  selection(): string;
  bracketed(): boolean;
  paste(target: PasteTarget, text: string): Promise<void>;
  control(target: PasteTarget, key: "C" | "V"): Promise<void>;
  focus(): void;
  selectAll(): void;
  clipboard: TextClipboard;
  copy(text: string, valid: () => boolean): Promise<boolean>;
}
export class TerminalClipboardController {
  state: ClipboardViewState = { ...emptyClipboardState };
  private serial = 0;
  private operation: {
    id: number;
    target: PasteTarget;
    phase: "reading" | "preview" | "submitting";
  } | null = null;
  private disposed = false;
  private noticeTimer: ReturnType<typeof setTimeout> | undefined;
  constructor(
    private env: ClipboardEnvironment,
    private changed: (state: ClipboardViewState) => void,
  ) {}
  private emit(patch: Partial<ClipboardViewState>) {
    if (this.disposed) return;
    this.state = { ...this.state, ...patch };
    this.changed(this.state);
  }
  refresh() {
    this.emit({
      hasSelection: !!this.env.selection(),
      canPaste: !!this.env.target(),
      unavailableReason: this.env.target() ? undefined : this.env.reason(),
    });
  }
  notice(text: string, kind: "success" | "error" | "info" = "info") {
    clearTimeout(this.noticeTimer);
    this.emit({ notice: { text, kind } });
    if (kind === "success")
      this.noticeTimer = setTimeout(
        () => this.emit({ notice: undefined }),
        1500,
      );
  }
  dismissNotice() {
    clearTimeout(this.noticeTimer);
    this.emit({ notice: undefined });
  }
  private valid(target: PasteTarget) {
    const now = this.env.target();
    return (
      !this.disposed &&
      !!now &&
      now.sessionId === target.sessionId &&
      now.epoch === target.epoch
    );
  }
  invalidate() {
    if (this.operation && this.operation.phase !== "submitting") {
      this.operation = null;
      this.emit({ busy: false, preview: undefined });
      this.notice("目标终端状态已改变，请重新粘贴");
    }
    this.refresh();
  }
  async copySelection(text = this.env.selection()) {
    if (!text || this.disposed) return;
    try {
      if (await this.env.copy(text, () => !this.disposed))
        this.notice("已复制", "success");
    } catch {
      this.notice("复制失败，请重试", "error");
    }
  }
  selectAllBuffer() {
    this.env.selectAll();
    this.refresh();
  }
  async sendControl(key: "C" | "V") {
    const target = this.env.target();
    if (!target || this.operation) {
      this.notice(this.operation ? "正在处理粘贴" : this.env.reason());
      return;
    }
    try {
      await this.env.control(target, key);
      if (this.valid(target)) this.env.focus();
    } catch {
      this.notice("发送按键失败，请检查终端连接", "error");
    }
  }
  async requestPaste(request: PasteRequest = {}) {
    if (this.operation) {
      this.notice("正在处理粘贴");
      return;
    }
    const target = this.env.target();
    if (!target) {
      this.notice(this.env.reason());
      return;
    }
    const op = {
      id: ++this.serial,
      target,
      phase: "reading" as "reading" | "preview" | "submitting",
    };
    this.operation = op;
    this.emit({ busy: true, notice: undefined });
    let text: string;
    try {
      text = request.text ?? (await this.env.clipboard.readText());
    } catch {
      if (this.operation === op) {
        this.finish(op.id);
        this.notice("无法读取系统剪贴板，请重试", "error");
      }
      return;
    }
    if (this.operation !== op) return;
    if (!this.valid(target)) {
      this.invalidate();
      return;
    }
    if (!text) {
      this.finish(op.id);
      this.notice("剪贴板中没有可粘贴的文本");
      return;
    }
    const analysis = analyzePaste(text);
    if (analysis.normalizedBytes > INPUT_LIMIT) {
      this.finish(op.id);
      this.notice("文本超过 1 MiB，请使用文件上传或远程编辑器", "error");
      return;
    }
    if (analysis.needsPreview || request.forcePreview) {
      op.phase = "preview";
      this.emit({ preview: { id: op.id, text, source: target.source } });
    } else await this.confirm(op.id, text);
  }
  async confirm(id: number, text: string) {
    const op = this.operation;
    if (!op || op.id !== id || op.phase === "submitting") return;
    if (!this.valid(op.target)) {
      this.invalidate();
      return;
    }
    const analysis = analyzePaste(text);
    if (
      !text ||
      analysis.controlCount ||
      finalPasteSize(text, this.env.bracketed()) > INPUT_LIMIT
    ) {
      this.notice(
        analysis.controlCount
          ? "请先移除控制字符"
          : !text
            ? "没有可粘贴的文本"
            : "文本超过 1 MiB，请使用文件上传或远程编辑器",
        "error",
      );
      // Direct single-line requests have no preview to return to.
      if (op.phase === "reading") this.finish(op.id);
      return;
    }
    op.phase = "submitting";
    this.emit({ preview: undefined });
    try {
      await this.env.paste(op.target, text);
      if (this.valid(op.target)) this.env.focus();
    } catch {
      this.notice(
        "粘贴未能确认完成，请检查终端内容和连接；不会自动重发",
        "error",
      );
    } finally {
      this.finish(op.id);
    }
  }
  cancel() {
    const op = this.operation;
    if (!op || op.phase === "submitting") return;
    this.finish(op.id);
    if (this.valid(op.target)) this.env.focus();
  }
  private finish(id: number) {
    if (this.operation?.id !== id) return;
    this.operation = null;
    this.emit({ busy: false, preview: undefined });
    this.refresh();
  }
  dispose() {
    this.disposed = true;
    this.operation = null;
    clearTimeout(this.noticeTimer);
  }
}
