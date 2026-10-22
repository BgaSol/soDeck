import { useCallback, useLayoutEffect, useRef, useState } from "react";

export function useTabStrip(activeId: string, layoutKey: string) {
  const root = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const track = useRef<HTMLDivElement>(null);
  const action = useRef<HTMLButtonElement>(null);
  const currentId = useRef(activeId);
  currentId.current = activeId;
  const [state, setState] = useState({
    overflowing: false,
    canScrollLeft: false,
    canScrollRight: false,
  });

  const revealTab = useCallback((id: string) => {
    const viewport = strip.current;
    const tab = Array.from(
      track.current?.querySelectorAll<HTMLElement>("[data-tab-id]") ?? [],
    ).find((el) => el.dataset.tabId === id);
    if (!viewport || !tab) return;
    const bounds = viewport.getBoundingClientRect();
    const item = tab.getBoundingClientRect();
    if (item.left < bounds.left) viewport.scrollLeft += item.left - bounds.left;
    else if (item.right > bounds.right)
      viewport.scrollLeft += item.right - bounds.right;
  }, []);

  const measure = useCallback(() => {
    if (!root.current || !strip.current || !track.current || !action.current)
      return;
    const gap = parseFloat(getComputedStyle(root.current).columnGap) || 0;
    // Compare against capacity BEFORE overflow controls. Their appearance must
    // not oscillate when they reduce the scroll viewport's width.
    const capacity =
      root.current.clientWidth - action.current.offsetWidth - gap;
    const overflowing =
      track.current.getBoundingClientRect().width > capacity + 1;
    const viewport = strip.current;
    // The strip is intrinsically sized. Include unused titlebar drag space so
    // an empty strip cannot pin the next tab's maximum width to zero.
    const dragRegion = root.current
      .closest(".window-titlebar")
      ?.querySelector<HTMLElement>(".window-drag-region");
    const reserved = dragRegion
      ? parseFloat(getComputedStyle(dragRegion).flexBasis) || 0
      : 0;
    const available =
      viewport.clientWidth +
      Math.max(0, (dragRegion?.clientWidth ?? 0) - reserved);
    track.current.style.setProperty("--tab-available-width", `${available}px`);
    const next = {
      overflowing,
      canScrollLeft: viewport.scrollLeft > 1,
      canScrollRight:
        viewport.scrollLeft + viewport.clientWidth < viewport.scrollWidth - 1,
    };
    setState((old) =>
      old.overflowing === next.overflowing &&
      old.canScrollLeft === next.canScrollLeft &&
      old.canScrollRight === next.canScrollRight
        ? old
        : next,
    );
  }, []);

  useLayoutEffect(() => {
    let frame = 0;
    const resize = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        measure();
        revealTab(currentId.current);
      });
    };
    const observer = new ResizeObserver(resize);
    [root.current, strip.current, track.current, action.current].forEach(
      (el) => el && observer.observe(el),
    );
    const viewport = strip.current!;
    const wheel = (event: WheelEvent) => {
      if (event.ctrlKey || viewport.scrollWidth <= viewport.clientWidth + 1)
        return;
      const raw =
        Math.abs(event.deltaX) > Math.abs(event.deltaY)
          ? event.deltaX
          : event.deltaY;
      const delta =
        raw *
        (event.deltaMode === 1
          ? 16
          : event.deltaMode === 2
            ? viewport.clientWidth
            : 1);
      if (
        (delta < 0 && viewport.scrollLeft > 0) ||
        (delta > 0 &&
          viewport.scrollLeft + viewport.clientWidth < viewport.scrollWidth)
      ) {
        event.preventDefault();
        viewport.scrollLeft += delta;
      }
    };
    viewport.addEventListener("scroll", measure);
    viewport.addEventListener("wheel", wheel, { passive: false });
    resize();
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      viewport.removeEventListener("scroll", measure);
      viewport.removeEventListener("wheel", wheel);
    };
  }, [measure, revealTab]);

  useLayoutEffect(() => {
    measure();
    const frame = requestAnimationFrame(() => revealTab(activeId));
    return () => cancelAnimationFrame(frame);
  }, [activeId, layoutKey, state.overflowing, measure, revealTab]);

  const scrollByPage = (direction: -1 | 1) =>
    strip.current?.scrollBy({
      left: direction * strip.current.clientWidth * 0.8,
      behavior: matchMedia("(prefers-reduced-motion: reduce)").matches
        ? "instant"
        : "smooth",
    });
  return { root, strip, track, action, ...state, scrollByPage, revealTab };
}
