import { useCallback, useEffect, useLayoutEffect, useRef, useState, type FocusEvent, type RefObject } from "react";

/** Where a card hangs: the viewport coordinates of its anchor's bottom-left corner, and of the anchor's top for flipping above. */
export interface CardAnchor {
  left: number;
  top: number;
  above: number;
}

/** Keeps a `position: fixed` card on screen: slides in from the right edge, flips above the anchor when there is no room below. Sets `data-side`. */
export function useCardPlacement(card: RefObject<HTMLElement | null>, at: CardAnchor | null) {
  useLayoutEffect(() => {
    const el = card.current;
    if (!at || !el) return;
    const { offsetWidth: w, offsetHeight: h } = el;
    el.style.left = `${Math.max(8, Math.min(at.left, window.innerWidth - w - 8))}px`;
    el.style.top = at.top + h > window.innerHeight - 8 && at.above - h > 8 ? `${at.above - h}px` : `${at.top}px`;
    el.dataset.side = el.style.top === `${at.top}px` ? "below" : "above";
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
