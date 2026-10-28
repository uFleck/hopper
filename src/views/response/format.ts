export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export function formatMs(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`;
}

export function timeAgo(iso: string, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - Date.parse(iso)) / 1000));
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86_400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86_400)} d ago`;
}

/** Size line: transferred vs decoded, only when they differ. */
export function sizeLabel(decoded: number, transferred?: number): string {
  if (transferred === undefined || transferred === decoded) return formatBytes(decoded);
  return `${formatBytes(transferred)} transferred, ${formatBytes(decoded)} decoded`;
}

export const headerValue = (headers: [string, string][], name: string): string | undefined =>
  headers.find(([k]) => k.toLowerCase() === name.toLowerCase())?.[1];

/** `image/png` from `image/png; charset=...`, or a fallback. */
export function mimeType(headers: [string, string][], fallback = "application/octet-stream"): string {
  return headerValue(headers, "content-type")?.split(";")[0].trim().toLowerCase() || fallback;
}

export const LARGE_BODY = 1_000_000;
export const CHUNK = 200_000;

/** The end of the next chunk of at most `size` characters from `from`, cut at a line break when there is one. */
export function chunkEnd(text: string, from: number, size = CHUNK): number {
  if (from + size >= text.length) return text.length;
  const nl = text.lastIndexOf("\n", from + size);
  return nl > from ? nl + 1 : from + size;
}
