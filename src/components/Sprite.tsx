/** Pixel art, drawn as crisp SVG rects. Rows are strings, one character per pixel, `.` is empty; colours come from the palette. */

export interface SpriteArt {
  pal: Record<string, string>;
  rows: string[];
}

export const SPRITES = {
  hopper: {
    pal: { k: "var(--hop-k)", L: "var(--hop-L)", G: "var(--hop-G)", D: "var(--hop-D)" },
    rows: ["kkkkkkkkkkkkkkkk", "kLLLLLLLLLLLLLGk", "kLDDDDDDDDDDDDGk", "kLDDDDDDDDDDDDGk", "kkGGGGGGGGGGGGkk", "kLGGGGGGGGGGGk", "kLGGGGGGGGGk", "kLGGGGGGGk", "kLGGGGGk", "kGGGGk", "kGGGGk", "kDDk", "kDDk", "kkkk"],
  },
  diamond: {
    pal: { c: "#4aedd9", w: "#d5fff8", d: "#1fa39a" },
    rows: ["...cc...", "..cwcc..", ".cwcccd.", "cwccccdd", ".ccccdd.", "..cccd..", "...cd...", "....d..."],
  },
  emerald: {
    pal: { c: "#2fdc6b", w: "#c4ffd8", d: "#0c8f3f" },
    rows: ["...cc...", "..cwcc..", ".cwcccd.", "cwccccdd", ".ccccdd.", "..cccd..", "...cd...", "....d..."],
  },
  gold: {
    pal: { c: "#f7c93a", w: "#fff3b0", d: "#b57d0b" },
    rows: ["...cc...", "..cwcc..", ".cwcccd.", "cwccccdd", ".ccccdd.", "..cccd..", "...cd...", "....d..."],
  },
  redstone: {
    pal: { c: "#e5392b", w: "#ff8a74", d: "#7d1209" },
    rows: ["...cc...", "..cwcc..", ".cwcccd.", "cwccccdd", ".ccccdd.", "..cccd..", "...cd...", "....d..."],
  },
} satisfies Record<string, SpriteArt>;

export type SpriteName = keyof typeof SPRITES;

export function Sprite({ name, size = 16, className }: { name: SpriteName; size?: number; className?: string }) {
  const art: SpriteArt = SPRITES[name];
  const w = Math.max(...art.rows.map((r) => r.length));
  const h = art.rows.length;
  return (
    <svg className={className} width={(size * w) / Math.max(w, h)} height={(size * h) / Math.max(w, h)} viewBox={`0 0 ${w} ${h}`} shapeRendering="crispEdges" aria-hidden="true" style={{ imageRendering: "pixelated", flex: "none" }}>
      {art.rows.flatMap((row, y) =>
        [...row].map((c, x) => (c === "." || !art.pal[c] ? null : <rect key={`${x}-${y}`} x={x + Math.floor((w - row.length) / 2)} y={y} width={1} height={1} fill={art.pal[c]} />)),
      )}
    </svg>
  );
}
