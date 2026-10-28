import { defaultKeymap } from "@codemirror/commands";
import { html } from "@codemirror/lang-html";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { bracketMatching, codeFolding, foldGutter, foldKeymap, HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from "@codemirror/search";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { useEffect, useImperativeHandle, useRef, type Ref } from "react";
import type { ContentKind } from "../../ipc";

const theme = EditorView.theme(
  {
    "&": { color: "var(--text-primary)", backgroundColor: "var(--bg-page)", fontSize: "12px", border: "1px solid var(--border-default)", borderRadius: "var(--r)", height: "100%" },
    "&.cm-focused": { outline: "none", boxShadow: "var(--ring)" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.5", overflow: "auto" },
    ".cm-content": { padding: "6px 0" },
    ".cm-line": { padding: "0 8px" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--sel-bg)" },
    ".cm-gutters": { backgroundColor: "var(--bg-page)", color: "var(--text-tertiary)", border: "none", borderRight: "1px solid var(--border-default)" },
    ".cm-foldPlaceholder": { backgroundColor: "var(--accent-soft)", border: "none", color: "var(--accent-text)" },
    ".cm-matchingBracket": { backgroundColor: "var(--accent-soft)", outline: "none" },
    ".cm-searchMatch": { backgroundColor: "var(--accent-soft)", outline: "1px solid var(--accent)" },
    ".cm-searchMatch-selected": { backgroundColor: "var(--sel-bg)" },
    ".cm-panels": { backgroundColor: "var(--bg-elevated)", color: "var(--text-primary)", borderColor: "var(--border-default)" },
    ".cm-panels input, .cm-panels button": { font: "inherit", fontSize: "12px" },
  },
  { dark: true },
);

const highlight = HighlightStyle.define([
  { tag: [t.propertyName, t.attributeName], color: "var(--i1)" },
  { tag: [t.string, t.attributeValue], color: "var(--i2)" },
  { tag: [t.number, t.bool, t.null], color: "var(--i3)" },
  { tag: [t.keyword, t.tagName, t.typeName], color: "var(--i4)" },
  { tag: [t.punctuation, t.separator, t.bracket, t.angleBracket], color: "var(--text-secondary)" },
  { tag: [t.comment, t.meta], color: "var(--text-tertiary)" },
]);

const lang = (k: ContentKind | "xml"): Extension => (k === "json" ? json() : k === "html" ? html() : k === "xml" ? xml() : []);

export interface CodeViewHandle {
  search: () => void;
}

/** A read-only CodeMirror view with folding and in-body search (Ctrl+F). */
export function CodeView({ value, kind, wrap, ref }: { value: string; kind: ContentKind | "xml"; wrap: boolean; ref?: Ref<CodeViewHandle> }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  useImperativeHandle(ref, () => ({ search: () => view.current && (view.current.focus(), openSearchPanel(view.current)) }), []);

  useEffect(() => {
    const v = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: [
          theme, syntaxHighlighting(highlight), lineNumbers(), codeFolding(), foldGutter(), bracketMatching(), highlightSelectionMatches(),
          search({ top: true }), keymap.of([...searchKeymap, ...foldKeymap, ...defaultKeymap]),
          EditorState.readOnly.of(true), EditorView.editable.of(true), lang(kind), wrap ? EditorView.lineWrapping : [],
          EditorView.contentAttributes.of({ "aria-label": "Response body", "aria-readonly": "true" }),
        ],
      }),
    });
    view.current = v;
    return () => v.destroy();
  }, [value, kind, wrap]);

  return <div ref={host} className="rv-code" />;
}
