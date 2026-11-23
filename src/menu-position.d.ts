export interface MenuPosition {
  ready: Promise<boolean>;
  update(): Promise<void>;
  dispose(): void;
}
export type MenuReference = HTMLElement | { x: number; y: number; contextElement: HTMLElement };
export interface MenuLayout {x:number;y:number;width:number;height:number;placement:string;reference:{left:number;right:number;top:number;bottom:number}}
interface PositionOptions {
  kind?: 'menu' | 'submenu';
  isValid?: () => boolean;
  onInvalid?: () => void;
}
export function positionMenu(menu:HTMLElement,reference:MenuReference,options?:PositionOptions&{onPosition?:(layout:MenuLayout)=>void}):MenuPosition;
export function attachMenuPosition(menu: HTMLElement, reference: MenuReference, options?: PositionOptions): MenuPosition;
declare global {
  interface Window {
    SoDeckMenuPosition: { attachMenuPosition: typeof attachMenuPosition };
  }
}
