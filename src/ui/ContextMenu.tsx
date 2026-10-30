import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type MouseEvent as ReactMouseEvent } from "react";
import { MenuList, type MenuItem } from "../views/find/Menu";

const WIDTH = 240;
const MARGIN = 8;

/** Start position for a box of `size` at `at`, so it stays `MARGIN` px inside a viewport of `viewport` px. */
export function clampToViewport(at: number, size: number, viewport: number): number {
  return Math.max(MARGIN, Math.min(at, viewport - size - MARGIN));
}

export interface ContextMenuProps {
  /** Client coordinates of the click. */
  x: number;
  y: number;
  items: MenuItem[];
  label?: string;
  onClose: () => void;
}

/** A menu at a point, for right-click. Closes on a press outside, Esc, scroll, window blur or a pick. Kept inside the viewport. */
export function ContextMenu({ x, y, items, label = "Context menu", onClose }: ContextMenuProps) {
  const list = useRef<HTMLDivElement | null>(null);
  // Focus goes back here when Esc closes the menu.
  const returnTo = useRef<HTMLElement | null>(document.activeElement as HTMLElement | null);
  const [style, setStyle] = useState<CSSProperties>({ left: x, top: y, width: WIDTH, maxHeight: window.innerHeight - MARGIN * 2 });

  useLayoutEffect(() => {
    const el = list.current;
    if (!el) return;
    const width = Math.min(WIDTH, window.innerWidth - MARGIN * 2);
    const maxHeight = window.innerHeight - MARGIN * 2;
    const height = Math.min(el.offsetHeight, maxHeight);
    setStyle({ left: clampToViewport(x, width, window.innerWidth), top: clampToViewport(y, height, window.innerHeight), width, maxHeight });
  }, [x, y]);

  useEffect(() => {
    const outside = (e: Event) => {
      if (!list.current?.contains(e.target as Node)) onClose();
    };
    const blur = () => onClose();
    document.addEventListener("mousedown", outside);
    window.addEventListener("scroll", outside, true);
    window.addEventListener("blur", blur);
    return () => {
      document.removeEventListener("mousedown", outside);
      window.removeEventListener("scroll", outside, true);
      window.removeEventListener("blur", blur);
    };
  }, [onClose]);

  const close = (refocus: boolean) => {
    if (refocus) returnTo.current?.focus();
    onClose();
  };

  return <MenuList items={items} label={label} listRef={list} style={style} onClose={close} />;
}

/** Right-click on a row: `onContextMenu` goes on the element, `menu` is rendered next to it. Calls preventDefault so the global listener leaves it alone. */
export function useContextMenu(items: MenuItem[], label?: string) {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const onContextMenu = (e: ReactMouseEvent) => {
    e.preventDefault();
    setAt({ x: e.clientX, y: e.clientY });
  };
  const menu = at && items.length > 0 ? <ContextMenu x={at.x} y={at.y} items={items} label={label} onClose={() => setAt(null)} /> : null;
  return { onContextMenu, menu };
}
