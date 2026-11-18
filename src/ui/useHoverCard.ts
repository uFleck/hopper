import { useCallback, useEffect, useLayoutEffect, useRef, useState, type FocusEvent, type RefObject } from "react";

/** Where a card hangs: the viewport coordinates of its anchor's bottom-left corner, and of the anchor's top for flipping above. */
export interface CardAnchor {
  left: number;
  top: number;
  above: number;
}

/**
 * Where a `w`×`h` card goes in a `vw`×`vh` viewport: below the anchor, above it when there is no room below,
 * and when neither side has room, on the roomier side but slid fully on screen, even if it then covers the anchor.
 */
export function placeCard(at: CardAnchor, w: number, h: number, vw: number, vh: number, margin = 8): { left: number; top: number; side: "below" | "above" } {
  const left = Math.max(margin, Math.min(at.left, vw - w - margin));
  if (at.top + h <= vh - margin) return { left, top: at.top, side: "below" };
  if (at.above - h >= margin) return { left, top: at.above - h, side: "above" };
  return vh - at.top >= at.above ? { left, top: Math.max(margin, vh - h - margin), side: "below" } : { left, top: margin, side: "above" };
}

/** Keeps a `position: fixed` card on screen with `placeCard`, again whenever its size changes (a field in it grows a row). Sets `data-side`. */
export function useCardPlacement(card: RefObject<HTMLElement | null>, at: CardAnchor | null) {
  useLayoutEffect(() => {
    const el = card.current;
    if (!at || !el) return;
    const place = () => {
      const p = placeCard(at, el.offsetWidth, el.offsetHeight, window.innerWidth, window.innerHeight);
      el.style.left = `${p.left}px`;
      el.style.top = `${p.top}px`;
      el.dataset.side = p.side;
    };
    place();
    const resized = new ResizeObserver(place);
    resized.observe(el);
    return () => resized.disconnect();
  }, [at, card]);
}

/**
 * State of a card opened by hovering something. It closes a moment after the pointer leaves the target and the card,
 * and stays open while something inside the card has focus, so a field in it can be clicked and typed in.
 */
export function useHoverCard<T>(delay = 300, openDelay = 180) {
  const [value, setValue] = useState<T | null>(null);
  const card = useRef<HTMLDivElement>(null);
  const timer = useRef<number>(undefined);
  const opener = useRef<number>(undefined);

  const cancelClose = useCallback(() => window.clearTimeout(timer.current), []);
  const show = useCallback((next: T) => {
    window.clearTimeout(timer.current);
    window.clearTimeout(opener.current);
    setValue(next);
  }, []);
  /** Opens once the pointer has rested on the target for `openDelay`; moves to another target while open switch at once. */
  const showSoon = useCallback((next: T, switching = false) => {
    window.clearTimeout(timer.current);
    window.clearTimeout(opener.current);
    if (switching) setValue(next);
    else opener.current = window.setTimeout(() => setValue(next), openDelay);
  }, [openDelay]);
  const closeSoon = useCallback(() => {
    window.clearTimeout(timer.current);
    window.clearTimeout(opener.current);
    timer.current = window.setTimeout(() => {
      if (!card.current?.contains(document.activeElement)) setValue(null);
    }, delay);
  }, [delay]);
  const close = useCallback(() => {
    window.clearTimeout(timer.current);
    window.clearTimeout(opener.current);
    setValue(null);
  }, []);
  useEffect(() => () => (window.clearTimeout(timer.current), window.clearTimeout(opener.current)), []);
  // The card is fixed where its anchor was; once the page under it scrolls or resizes it points at the wrong text, so it goes.
  const open = value !== null;
  useEffect(() => {
    if (!open) return;
    const moved = (e: Event) => {
      if (!(e.target instanceof Node && card.current?.contains(e.target))) close();
    };
    window.addEventListener("scroll", moved, true);
    window.addEventListener("resize", moved);
    return () => (window.removeEventListener("scroll", moved, true), window.removeEventListener("resize", moved));
  }, [open, close]);

  /** Spread onto the card's element. */
  const cardProps = {
    ref: card,
    onMouseEnter: cancelClose,
    onMouseLeave: closeSoon,
    onBlur: (e: FocusEvent<HTMLElement>) => {
      if (!e.currentTarget.contains(e.relatedTarget)) closeSoon();
    },
  };

  return { value, show, showSoon, keep: cancelClose, closeSoon, close, cardProps };
}
