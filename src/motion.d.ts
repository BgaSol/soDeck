export function motionDuration(element: Element, kind?: string): number;
export function animateMotion(element: Element, frames: Keyframe[], kind?: string): Animation | undefined;
export function cancelMotion(element: Element): void;
export function observeMotion(document: Document): void;
export function revealMotion(element: Element): Animation | undefined;
export function enterMotion(element: Element, direction?: 'x' | 'y'): Animation | undefined;
export function exitMotion(element: Element): void;
export function feedbackMotion(element: Element): Animation | undefined;
export function setMotionText(element: Element, value: unknown): void;
export function captureMotion(root: Element, selector?: string): Map<Element, DOMRect>;
export function settleMotion(root: Element, before: Map<Element, DOMRect>, selector?: string): void;
declare global { interface Window { SoDeckMotion: {animateMotion:typeof animateMotion;cancelMotion:typeof cancelMotion;enterMotion:typeof enterMotion;exitMotion:typeof exitMotion;feedbackMotion:typeof feedbackMotion;setMotionText:typeof setMotionText;revealMotion:typeof revealMotion;captureMotion:typeof captureMotion;settleMotion:typeof settleMotion;observeMotion:typeof observeMotion}; } }
