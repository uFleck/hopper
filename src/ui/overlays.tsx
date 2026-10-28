import { useEffect, useRef, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { createStore, useStoreSelector } from "../store/core";
import { dismissToast } from "../store/actions";
import { useStore, type Toast } from "../store/state";
import { Button, Icon, IconButton, Spinner } from "./primitives";

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Keeps Tab inside `ref` while `active`, moves focus in on open and gives it back on close. */
export function useFocusTrap(ref: RefObject<HTMLElement | null>, active: boolean, initialFocus?: RefObject<HTMLElement | null>) {
  useEffect(() => {
    if (!active) return;
    const root = ref.current;
    if (!root) return;
    const before = document.activeElement as HTMLElement | null;
    (initialFocus?.current ?? root.querySelector<HTMLElement>(FOCUSABLE) ?? root).focus();
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const items = [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null);
      if (!items.length) return e.preventDefault();
      const first = items[0];
      const last = items[items.length - 1];
      const at = document.activeElement;
      if (e.shiftKey && (at === first || !root.contains(at))) (e.preventDefault(), last.focus());
      else if (!e.shiftKey && (at === last || !root.contains(at))) (e.preventDefault(), first.focus());
    };
    root.addEventListener("keydown", onKey);
    return () => {
      root.removeEventListener("keydown", onKey);
      if (before?.isConnected) before.focus();
    };
  }, [ref, active, initialFocus]);
}

/** Esc inside an overlay closes it and does not reach the panel's own Esc (which dismisses the whole panel). */
function escToClose(onClose: () => void) {
  return (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    onClose();
  };
}

// --------------------------------------------------------------------------------------------- sheet

export interface SheetProps {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Pinned under the content, e.g. Save / Cancel. */
  footer?: ReactNode;
  /** Shown next to the title. */
  actions?: ReactNode;
}

/** Slides over the panel (render it inside `.panel`). Esc or the back button closes it; Tab stays inside. */
export function Sheet({ open, title, onClose, children, footer, actions }: SheetProps) {
  const ref = useRef<HTMLElement>(null);
  useFocusTrap(ref, open);
  if (!open) return null;
  return (
    <section ref={ref} className="ui-sheet" role="dialog" aria-modal="true" aria-label={title} tabIndex={-1} onKeyDown={escToClose(onClose)} onMouseDown={(e) => e.stopPropagation()}>
      <header className="ui-sheet-head">
        <IconButton icon="arrow-left" label="Back" onClick={onClose} />
        <h2 className="ui-sheet-title">{title}</h2>
        {actions}
      </header>
      <div className="ui-sheet-body">{children}</div>
      {footer && <footer className="ui-sheet-foot">{footer}</footer>}
    </section>
  );
}

// ------------------------------------------------------------------------------------ confirm dialog

export interface ConfirmOptions {
  title: string;
  message?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  /** `danger` styles the confirm button red and focuses Cancel first. */
  tone?: "default" | "danger";
}

export interface ConfirmDialogProps extends ConfirmOptions {
  open: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** The in-app replacement for `window.confirm`. Never use native dialogs. */
export function ConfirmDialog({ open, title, message, confirmLabel = "Confirm", cancelLabel = "Cancel", tone = "default", busy, onConfirm, onCancel }: ConfirmDialogProps) {
  const ref = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  useFocusTrap(ref, open, tone === "danger" ? cancelRef : confirmRef);
  if (!open) return null;
  return (
    <div className="ui-scrim" onMouseDown={(e) => e.stopPropagation()}>
      <div ref={ref} className="ui-dialog" role="alertdialog" aria-modal="true" aria-label={title} onKeyDown={escToClose(onCancel)}>
        <h3 className="ui-dialog-title">{title}</h3>
        {message && <div className="ui-dialog-msg">{message}</div>}
        <div className="ui-dialog-actions">
          <Button ref={cancelRef} onClick={onCancel}>{cancelLabel}</Button>
          <Button ref={confirmRef} variant={tone === "danger" ? "danger" : "primary"} loading={busy} onClick={onConfirm}>{confirmLabel}</Button>
        </div>
      </div>
    </div>
  );
}

interface Pending extends ConfirmOptions {
  resolve: (ok: boolean) => void;
}
const pending = createStore<{ current: Pending | null }>({ current: null });

/** `if (await confirm({ title: "Delete request?", tone: "danger", confirmLabel: "Delete" })) ...`. Needs `<ConfirmHost />` in the panel. */
export function confirm(opts: ConfirmOptions): Promise<boolean> {
  pending.getState().current?.resolve(false);
  return new Promise((resolve) => pending.setState({ current: { ...opts, resolve } }));
}

export function ConfirmHost() {
  const current = useStoreSelector(pending, (s) => s.current);
  const done = (ok: boolean) => {
    current?.resolve(ok);
    pending.setState({ current: null });
  };
  return current ? <ConfirmDialog open {...current} onConfirm={() => done(true)} onCancel={() => done(false)} /> : null;
}

// ------------------------------------------------------------------------------------------- toasts

const TOAST_ICON = { info: "info", success: "check-circle", error: "warning-circle" } as const;

/** Renders the store's toasts (`toast()` and the undo toasts of `deleteRequest`) at the bottom of the panel. */
export function ToastHost() {
  const toasts = useStore((s) => s.toasts);
  return (
    <div className="ui-toasts" role="region" aria-label="Notifications" aria-live="polite">
      {toasts.map((t) => (
        <ToastItem key={t.id} {...t} />
      ))}
    </div>
  );
}

function ToastItem({ id, kind, message, undo, ttl }: Toast) {
  useEffect(() => {
    if (!ttl) return;
    const t = setTimeout(() => dismissToast(id), ttl);
    return () => clearTimeout(t);
  }, [id, ttl]);
  return (
    <div className={`ui-toast ui-toast--${kind}`}>
      <Icon name={TOAST_ICON[kind]} />
      <span className="ui-toast-msg">{message}</span>
      {undo && (
        <button type="button" className="ui-toast-undo" onClick={() => { void undo.run(); dismissToast(id); }}>
          {undo.label}
        </button>
      )}
      <IconButton icon="x" label="Dismiss" size="sm" onClick={() => dismissToast(id)} />
    </div>
  );
}

/** A loading placeholder row. */
export function Busy({ label = "Loading" }: { label?: string }) {
  return (
    <div className="ui-busy">
      <Spinner /> <span>{label}</span>
    </div>
  );
}
