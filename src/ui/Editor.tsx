import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap, type CompletionContext } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { html } from "@codemirror/lang-html";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { bracketMatching, HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, Facet, RangeSetBuilder, type Extension } from "@codemirror/state";
import { Decoration, drawSelection, EditorView, highlightActiveLine, keymap, lineNumbers, placeholder as cmPlaceholder, ViewPlugin, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useHoverCard } from "./useHoverCard";

export type EditorLanguage = "json" | "xml" | "html" | "text";

export interface VariableOption {
  name: string;
  /** Shown next to the name in the list, e.g. the value or "secret". */
  detail?: string;
}

export interface EditorProps {
  value: string;
  onChange?: (value: string) => void;
  language?: EditorLanguage;
  readOnly?: boolean;
  /** Names that resolve; any other `{{name}}` is underlined red. Leave out to skip the check. */
  known?: Set<string>;
  /** Offered after typing `{{`. */
  variables?: VariableOption[];
  /** Hovering any `{{name}}` opens a card with this content; it stays open while its fields have focus. Leave out for no card. */
  renderVariableTip?: (name: string) => ReactNode;
  /** Ctrl+S (Cmd+S). */
  onSave?: () => void;
  placeholder?: string;
  lineNumbers?: boolean;
  /** Wrap long lines (default), so 430 px never scrolls sideways. */
  wrap?: boolean;
  /** Pixels. The editor grows to fit up to `maxHeight`, then scrolls. */
  minHeight?: number;
  maxHeight?: number;
  ariaLabel?: string;
  className?: string;
}

// ----------------------------------------------------------------------------------- {{variables}}

const knownFacet = Facet.define<Set<string> | undefined, Set<string> | undefined>({ combine: (v) => v[0] });
const variablesFacet = Facet.define<VariableOption[], VariableOption[]>({ combine: (v) => v[0] ?? [] });

const VAR_RE = /\{\{\s*([\w.-]*)\s*\}\}/g;
const varOk = Decoration.mark({ class: "cm-var" });
const varBad = Decoration.mark({ class: "cm-var cm-var-unresolved" });

function varDecorations(view: EditorView): DecorationSet {
  const known = view.state.facet(knownFacet);
  const b = new RangeSetBuilder<Decoration>();
  for (const { from, to } of view.visibleRanges) {
    const text = view.state.sliceDoc(from, to);
    for (const m of text.matchAll(VAR_RE)) {
      const start = from + (m.index ?? 0);
      b.add(start, start + m[0].length, known && !known.has(m[1]) ? varBad : varOk);
    }
  }
  return b.finish();
}

const variableHighlighter = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = varDecorations(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged || u.startState.facet(knownFacet) !== u.state.facet(knownFacet)) this.decorations = varDecorations(u.view);
    }
  },
  { decorations: (v) => v.decorations },
);

/** The `{{name}}` token around `pos` and where it starts, or null. */
function variableAround(view: EditorView, pos: number): { name: string; from: number } | null {
  const line = view.state.doc.lineAt(pos);
  for (const m of line.text.matchAll(VAR_RE)) {
    const from = line.from + (m.index ?? 0);
    if (pos >= from && pos <= from + m[0].length) return m[1] ? { name: m[1], from } : null;
  }
  return null;
}

interface VarHover {
  name: string;
  from: number;
  /** Viewport coordinates of the token's bottom-left corner. */
  left: number;
  top: number;
  /** Viewport y of the token's top, for placing the card above it. */
  above: number;
}

function variableCompletions(ctx: CompletionContext) {
  const open = ctx.matchBefore(/\{\{\s*[\w.-]*/);
  if (!open) return null;
  const word = /[\w.-]*$/.exec(open.text)![0];
  const vars = ctx.state.facet(variablesFacet);
  if (!vars.length) return null;
  const closed = ctx.state.sliceDoc(ctx.pos, ctx.pos + 2) === "}}";
  return {
    from: open.to - word.length,
    validFor: /^[\w.-]*$/,
    options: vars.map((v) => ({ label: v.name, detail: v.detail, type: "variable", apply: closed ? v.name : `${v.name}}}` })),
  };
}

// ----------------------------------------------------------------------------------------- theme

const theme = EditorView.theme(
  {
    "&": { color: "var(--text-primary)", backgroundColor: "var(--bg-page)", fontSize: "13.5px", border: "1px solid var(--border-default)", borderRadius: "var(--r)" },
    "&.cm-focused": { outline: "none", boxShadow: "var(--ring)" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.5", overflow: "auto" },
    ".cm-content": { caretColor: "var(--accent)", padding: "6px 0" },
    ".cm-line": { padding: "0 8px" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--sel-bg)" },
    ".cm-activeLine": { backgroundColor: "var(--hover-tint)" },
    ".cm-gutters": { backgroundColor: "var(--bg-page)", color: "var(--text-tertiary)", border: "none", borderRight: "1px solid var(--border-default)" },
    ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--text-secondary)" },
    ".cm-placeholder": { color: "var(--text-tertiary)" },
    ".cm-matchingBracket": { backgroundColor: "var(--accent-soft)", outline: "none" },
    ".cm-searchMatch": { backgroundColor: "var(--accent-soft)", outline: "1px solid var(--accent)" },
    ".cm-panels": { backgroundColor: "var(--bg-elevated)", color: "var(--text-primary)", borderColor: "var(--border-default)" },
    ".cm-panels input, .cm-panels button": { font: "inherit", fontSize: "13.5px" },
    ".cm-tooltip": { backgroundColor: "var(--bg-elevated)", border: "1px solid var(--border-strong)", borderRadius: "var(--r)", boxShadow: "var(--shadow-pop)", color: "var(--text-primary)" },
    ".cm-tooltip-autocomplete > ul": { fontFamily: "var(--font-mono)", fontSize: "13.5px", maxHeight: "180px" },
    ".cm-tooltip-autocomplete ul li[aria-selected]": { backgroundColor: "var(--sel-bg)", color: "var(--sel-fg)" },
    ".cm-completionDetail": { color: "var(--text-tertiary)", fontStyle: "normal", marginLeft: "8px" },
    ".cm-var": { color: "var(--accent-text)", backgroundColor: "var(--accent-soft)", borderRadius: "5px", padding: "1px 2px", margin: "0 -2px", cursor: "default", transition: "background-color 120ms ease, box-shadow 120ms ease" },
    ".cm-var:hover": { backgroundColor: "color-mix(in srgb, var(--accent) 30%, transparent)", boxShadow: "0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent)" },
    ".cm-var-unresolved": { color: "var(--err)", backgroundColor: "color-mix(in srgb, var(--err) 10%, transparent)", textDecoration: "underline wavy var(--err)", textUnderlineOffset: "3px" },
  },
  { dark: true },
);

const highlight = HighlightStyle.define([
  { tag: [t.propertyName, t.attributeName], color: "var(--i1)" },
  { tag: [t.string, t.attributeValue], color: "var(--i2)" },
  { tag: [t.number, t.bool, t.null], color: "var(--i3)" },
  { tag: [t.keyword, t.tagName, t.typeName], color: "var(--i4)" },
  { tag: [t.punctuation, t.separator, t.bracket, t.angleBracket], color: "var(--text-secondary)" },
  { tag: [t.comment, t.meta, t.processingInstruction], color: "var(--text-tertiary)" },
  { tag: t.invalid, color: "var(--err)" },
]);

function languageExt(lang: EditorLanguage): Extension {
  return lang === "json" ? json() : lang === "xml" ? xml() : lang === "html" ? html() : [];
}

// --------------------------------------------------------------------------------------- component

/** A themed CodeMirror 6 editor. Controlled by `value`; `{{variables}}` are highlighted and offered on `{{`. */
export function Editor({
  value, onChange, language = "text", readOnly = false, known, variables, renderVariableTip, onSave, placeholder, lineNumbers: gutter = true,
  wrap = true, minHeight = 80, maxHeight, ariaLabel, className,
}: EditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const hover = useHoverCard<VarHover>();
  const cb = useRef({ onChange, onSave, renderVariableTip, hover });
  cb.current = { onChange, onSave, renderVariableTip, hover };
  const card = hover.cardProps.ref;
  const at = hover.value;
  // Keep the card on screen: slide it in from the right edge, and flip it above the token when there is no room below.
  useLayoutEffect(() => {
    const el = card.current;
    if (!at || !el) return;
    const { offsetWidth: w, offsetHeight: h } = el;
    el.style.left = `${Math.max(8, Math.min(at.left, window.innerWidth - w - 8))}px`;
    const below = at.top;
    el.style.top = below + h > window.innerHeight - 8 && at.above - h > 8 ? `${at.above - h}px` : `${below}px`;
    el.dataset.side = el.style.top === `${below}px` ? "below" : "above";
  }, [at, card]);
  const compartments = useRef({
    language: new Compartment(), editable: new Compartment(), known: new Compartment(), vars: new Compartment(),
    wrap: new Compartment(), gutter: new Compartment(),
  });

  useEffect(() => {
    const c = compartments.current;
    const v = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: [
          theme,
          syntaxHighlighting(highlight),
          history(),
          drawSelection(),
          highlightActiveLine(),
          bracketMatching(),
          closeBrackets(),
          search({ top: true }),
          highlightSelectionMatches(),
          autocompletion({ override: [variableCompletions], icons: false }),
          variableHighlighter,
          EditorView.domEventHandlers({
            mousemove(e, v) {
              const { renderVariableTip: render, hover: h } = cb.current;
              if (!render) return;
              const pos = v.posAtCoords({ x: e.clientX, y: e.clientY });
              const hit = pos === null ? null : variableAround(v, pos);
              const coords = hit && v.coordsAtPos(hit.from);
              if (!hit || !coords) {
                h.closeSoon();
                return;
              }
              if (h.value?.from === hit.from && h.value.name === hit.name) h.keep();
              else h.showSoon({ ...hit, left: coords.left, top: coords.bottom, above: coords.top }, !!h.value);
            },
            mouseleave: () => void (cb.current.hover.value && cb.current.hover.closeSoon()),
            keydown: () => cb.current.hover.close(),
          }),
          keymap.of([
            { key: "Mod-s", preventDefault: true, run: () => (cb.current.onSave?.(), true) },
            ...closeBracketsKeymap, ...defaultKeymap, ...searchKeymap, ...historyKeymap, ...completionKeymap, indentWithTab,
          ]),
          c.language.of(languageExt(language)),
          c.editable.of([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
          c.known.of(knownFacet.of(known)),
          c.vars.of(variablesFacet.of(variables ?? [])),
          c.wrap.of(wrap ? EditorView.lineWrapping : []),
          c.gutter.of(gutter ? lineNumbers() : []),
          placeholder ? cmPlaceholder(placeholder) : [],
          EditorView.contentAttributes.of({ "aria-label": ariaLabel ?? "Editor" }),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged) return;
            cb.current.hover.close();
            cb.current.onChange?.(u.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = v;
    return () => {
      v.destroy();
      view.current = null;
    };
    // The view is created once; props reach it through the effects below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== value) v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
  }, [value]);

  useEffect(() => {
    const c = compartments.current;
    view.current?.dispatch({
      effects: [
        c.language.reconfigure(languageExt(language)),
        c.editable.reconfigure([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
        c.known.reconfigure(knownFacet.of(known)),
        c.vars.reconfigure(variablesFacet.of(variables ?? [])),
        c.wrap.reconfigure(wrap ? EditorView.lineWrapping : []),
        c.gutter.reconfigure(gutter ? lineNumbers() : []),
      ],
    });
  }, [language, readOnly, known, variables, wrap, gutter]);

  return (
    <>
      <div
        ref={host}
        className={["ui-editor", className].filter(Boolean).join(" ")}
        style={{ ["--ed-min" as string]: `${minHeight}px`, ["--ed-max" as string]: maxHeight ? `${maxHeight}px` : "none" }}
      />
      {hover.value && renderVariableTip &&
        createPortal(
          <div {...hover.cardProps} className="vp-float" style={{ left: hover.value.left, top: hover.value.top }}>
            <div key={`${hover.value.name}@${hover.value.from}`}>{renderVariableTip(hover.value.name)}</div>
          </div>,
          document.body,
        )}
    </>
  );
}
