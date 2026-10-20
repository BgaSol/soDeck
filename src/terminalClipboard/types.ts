export type TerminalPlatform = "windows" | "linux" | "macos";
export interface TerminalClipboardPreferences {
  shortcutMode: "platform" | "convenient" | "traditional";
  copyOnSelect: boolean;
  rightClickPaste: boolean;
}
export const defaultPreferences: TerminalClipboardPreferences = {
  shortcutMode: "platform",
  copyOnSelect: false,
  rightClickPaste: false,
};
export interface TerminalSourceLabel {
  serverName: string;
  host: string;
  port: number;
  username: string;
  containerName?: string;
}
export interface PasteTarget {
  sessionId: string;
  epoch: number;
  source: TerminalSourceLabel;
}
export type PasteRequest = { text?: string; forcePreview?: boolean };
export interface ClipboardActions {
  copySelection(): Promise<void>;
  requestPaste(request?: PasteRequest): Promise<void>;
  selectAllBuffer(): void;
  sendControl(key: "C" | "V"): Promise<void>;
}
export interface ClipboardViewState {
  hasSelection: boolean;
  canPaste: boolean;
  busy: boolean;
  unavailableReason?: string;
  notice?: { kind: "success" | "error" | "info"; text: string };
  preview?: { id: number; text: string; source: TerminalSourceLabel };
}
export const emptyClipboardState: ClipboardViewState = {
  hasSelection: false,
  canPaste: false,
  busy: false,
};
