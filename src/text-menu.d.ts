export interface TextMenu {
  close(focus?: boolean): void;
  dispose(): void;
}
declare global {
  interface Window {
    installTextMenu(options: {
      read: (valid: () => boolean, source?: Element) => Promise<string | null>;
      write: (text: string, valid: () => boolean, source?: Element) => Promise<boolean | void>;
      notify: (error: string) => void;
    }): TextMenu;
  }
}
