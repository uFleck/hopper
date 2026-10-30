import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { clampToViewport } from "./ContextMenu";

const DELAY = 500;
const GAP = 6;
const MARGIN = 8;

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Centred under the anchor, or above it when there is no room below. Kept inside the viewport. */
export function placeTooltip(anchor: Box, size: { width: number; height: number }, viewport: { width: number; height: number }) {
  const left = clampToViewport(anchor.left + anchor.width / 2 - size.width / 2, size.width, viewport.width);
  const below = anchor.top + anchor.height + GAP;
  const top = below + size.height <= viewport.height - MARGIN ? below : Math.max(MARGIN, anchor.top - GAP - size.height);
  return { left, top };
}

/** One host for every `data-tip`: a delegated listener reads the closest `[data-tip]` under the pointer. Mount it once. */
export function Tooltip() {
  const [tip, setTip] = useState<{ text: string; anchor: Box } | null>(null);
  const [place, setPlace] = useState<{ left: number; top: number } | null>(null);
  const box = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let current: HTMLElement | null = null;
    let timer = 0;
    const hide = () => {
      window.clearTimeout(timer);
      setTip(null);
      setPlace(null);
    };
    // Hiding on press keeps it hidden until the pointer moves to another element.
    const over = (e: MouseEvent) => {
      const el = e.target instanceof Element ? e.target.closest<HTMLElement>("[data-tip]") : null;
      if (el === current) return;
      current = el;
      hide();
      const text = el?.dataset.tip;
      if (!el || !text) return;
      timer = window.setTimeout(() => setTip({ text, anchor: el.getBoundingClientRect() }), DELAY);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") hide();
    };
    document.addEventListener("mouseover", over);
    document.addEventListener("pointerdown", hide, true);
    window.addEventListener("scroll", hide, true);
    window.addEventListener("blur", hide);
    window.addEventListener("keydown", key);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("mouseover", over);
      document.removeEventListener("pointerdown", hide, true);
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("blur", hide);
      window.removeEventListener("keydown", key);
    };
  }, []);

  useLayoutEffect(() => {
    if (!tip || !box.current) return;
    const { width, height } = box.current.getBoundingClientRect();
    setPlace(placeTooltip(tip.anchor, { width, height }, { width: window.innerWidth, height: window.innerHeight }));
  }, [tip]);

  if (!tip) return null;
  return (
    <div ref={box} className="ui-tip" role="tooltip" style={{ left: place?.left ?? 0, top: place?.top ?? 0, visibility: place ? "visible" : "hidden" }}>
      {tip.text}
    </div>
  );
}
