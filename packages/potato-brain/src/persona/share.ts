import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, resolve } from "node:path";

export interface ShareItem {
  /** absolute path, guaranteed to be inside the share root */
  path: string;
  /** file name, the model refers to it by this */
  name: string;
  kind: "image" | "video" | "other";
  tags: string[];
  note: string;
  mtime: number;
}

const IMAGES = new Set([".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]);
const VIDEOS = new Set([".mp4", ".webm", ".mov", ".mkv"]);

function kindOf(name: string): ShareItem["kind"] {
  const ext = extname(name).toLowerCase();
  if (IMAGES.has(ext)) return "image";
  if (VIDEOS.has(ext)) return "video";
  return "other";
}

/**
 * index.md, one item per line:
 *
 *   - cat-fail.gif | 貓,笑死,智障 | 貓咪跳失敗，笑死
 *
 * Files with no line still count, and use their filename as the note.
 * The index is optional so dropping a file in the folder just works.
 */
function readIndex(root: string): Map<string, { tags: string[]; note: string }> {
  const out = new Map<string, { tags: string[]; note: string }>();
  const path = join(root, "index.md");
  if (!existsSync(path)) return out;

  let text = "";
  try {
    text = readFileSync(path, "utf8");
  } catch {
    return out;
  }

  for (const raw of text.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trim();
    if (!line.startsWith("- ")) continue;
    const [file, tags, note] = line.slice(2).split("|").map((s) => s.trim());
    if (!file) continue;
    out.set(file, {
      tags: (tags ?? "")
        .split(/[,，、\s]+/)
        .map((t) => t.trim())
        .filter(Boolean)
        .slice(0, 8),
      note: (note ?? "").slice(0, 80),
    });
  }
  return out;
}

export function loadShare(root: string): ShareItem[] {
  if (!existsSync(root)) return [];

  const index = readIndex(root);
  const base = resolve(root);

  let names: string[] = [];
  try {
    names = readdirSync(root);
  } catch {
    return [];
  }

  const items: ShareItem[] = [];
  for (const name of names) {
    if (name === "index.md" || name.startsWith(".")) continue;
    const abs = resolve(base, name);
    if (!abs.startsWith(base)) continue;

    let stat;
    try {
      stat = statSync(abs);
    } catch {
      continue;
    }
    if (!stat.isFile()) continue;

    const meta = index.get(name);
    items.push({
      path: abs,
      name,
      kind: kindOf(name),
      tags: meta?.tags ?? [],
      note: meta?.note ?? name,
      mtime: stat.mtimeMs,
    });
  }

  items.sort((a, b) => a.mtime - b.mtime);
  return items;
}

/**
 * Tag overlap wins; ties go to the file she has not sent recently;
 * nothing matched -> the oldest thing she has not sent yet.
 */
export function pickShare(
  items: ShareItem[],
  query: string,
  seed: number,
  recent: readonly string[] = [],
): ShareItem | null {
  if (items.length === 0) return null;

  const q = query.trim().toLowerCase();
  let best: ShareItem | null = null;
  let bestScore = 0;

  for (const item of items) {
    const hay = `${item.tags.join(" ")} ${item.note} ${item.name}`.toLowerCase();
    let score = 0;
    for (const tag of item.tags) {
      if (tag !== "" && q.includes(tag.toLowerCase())) score += 4;
    }
    for (const ch of new Set([...q])) {
      if (ch.trim() !== "" && hay.includes(ch)) score += 1;
    }
    if (recent.includes(item.name)) score -= 3;
    if (score > bestScore) {
      best = item;
      bestScore = score;
    }
  }

  if (best) return best;

  const pool = items.filter((item) => !recent.includes(item.name));
  const arr = pool.length > 0 ? pool : items;
  return arr[Math.abs(Math.trunc(seed)) % arr.length] ?? null;
}
