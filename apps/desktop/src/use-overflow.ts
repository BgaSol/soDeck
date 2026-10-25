import { useEffect, useState, type RefObject } from 'react';
export function useOverflow(ref: RefObject<HTMLElement | null>, contentKey: string) {
  const [overflow, setOverflow] = useState(false);
  useEffect(() => {
    const row = ref.current; if (!row) return;
    const measure = () => setOverflow(row.scrollWidth > row.clientWidth + 1);
    const observer = new ResizeObserver(measure); observer.observe(row);
    for (const child of row.children) observer.observe(child);
    measure(); return () => observer.disconnect();
  }, [ref, contentKey]);
  return overflow;
}
