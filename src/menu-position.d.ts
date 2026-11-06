export interface MenuPosition {
  ready: Promise<boolean>;
  update(): Promise<void>;
  dispose(): void;
}
export type MenuReference = HTMLElement | { x: number; y: number; contextElement: HTMLElement };
export function attachMenuPosition(menu: HTMLElement, reference: MenuReference, options?: {
  kind?: 'menu' | 'submenu';
  isValid?: () => boolean;
  onInvalid?: () => void;
}): MenuPosition;
declare global {
  interface Window {
    SoDeckMenuPosition: { attachMenuPosition: typeof attachMenuPosition };
  }
}
