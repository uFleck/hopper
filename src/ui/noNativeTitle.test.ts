import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// Components whose `title` prop is text (a heading or dialog title), not a DOM attribute.
const ALLOWED_COMPONENTS = new Set(["Sheet", "Section", "ConfirmDialog", "WorkspaceList"]);

const SRC = fileURLToPath(new URL("..", import.meta.url));

function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return walk(path);
    return entry.name.endsWith(".tsx") && !/\.test\.tsx$/.test(entry.name) ? [path] : [];
  });
}

/** Attribute text of every JSX opening tag, with the tag name and the offset of its `<`. Braces are tracked so `=>` inside `{}` does not end the tag. */
function jsxTags(source: string): { name: string; at: number; attrs: string }[] {
  const tags: { name: string; at: number; attrs: string }[] = [];
  for (const m of source.matchAll(/<([A-Za-z][\w.-]*)/g)) {
    const start = (m.index ?? 0) + m[0].length;
    let depth = 0;
    let quote: string | null = null;
    let end = start;
    for (; end < source.length; end++) {
      const ch = source[end];
      if (quote) {
        if (ch === quote) quote = null;
      } else if (ch === '"' || ch === "'" || ch === "`") {
        if (depth === 0 || ch === '"') quote = ch;
      } else if (ch === "{") depth++;
      else if (ch === "}") depth--;
      else if (ch === ">" && depth === 0) break;
    }
    // Keep only the attributes at brace depth 0 so a `title=` inside an expression is ignored.
    let attrs = "";
    let d = 0;
    for (let i = start; i < end; i++) {
      const ch = source[i];
      if (ch === "{") d++;
      else if (ch === "}") d--;
      else if (d === 0) attrs += ch;
      else attrs += " ";
    }
    tags.push({ name: m[1], at: m.index ?? 0, attrs });
  }
  return tags;
}

describe("no native title tooltips", () => {
  it("uses data-tip (or tip on Chip) instead of title", () => {
    const files = walk(SRC);
    expect(files.length).toBeGreaterThan(0);

    const hits: string[] = [];
    for (const file of files) {
      const source = readFileSync(file, "utf8");
      for (const tag of jsxTags(source)) {
        if (!/(^|\s)title=/.test(tag.attrs)) continue;
        const isDom = tag.name[0] === tag.name[0].toLowerCase();
        if (!isDom && ALLOWED_COMPONENTS.has(tag.name)) continue;
        const line = source.slice(0, tag.at).split("\n").length;
        hits.push(`${file.slice(SRC.length).replace(/\\/g, "/")}:${line} <${tag.name} title=`);
      }
    }

    if (hits.length > 0) {
      throw new Error(`use data-tip, native title tooltips are banned:\n${hits.join("\n")}`);
    }
  });
});
