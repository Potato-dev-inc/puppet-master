import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import type { Block } from "../types.ts";
import type { Hit } from "./types.ts";
import { MemoryIndex } from "./index.ts";
import { dayKey, journalPath, listMarkdown, readDoc, readWindow } from "./vault.ts";
import type { Jot } from "./vault.ts";

function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

function minutesBetween(a: string, b: string): number {
  const [ah, am] = a.split(":").map(Number);
  const [bh, bm] = b.split(":").map(Number);
  const diff = Math.abs((bh ?? 0) * 60 + (bm ?? 0) - ((ah ?? 0) * 60 + (am ?? 0)));
  return Math.min(diff, 1440 - diff);
}

function parseHeardLine(line: string): { hhmm: string; where: string; speaker: string } | null {
  const m = /^- (\d{2}:\d{2})\s*(（[^）]*）)?(.+?)：/.exec(line);
  if (!m) return null;
  return { hhmm: m[1] ?? "", where: m[2] ?? "", speaker: m[3] ?? "" };
}

export interface PersonProfile {
  id: string;
  name: string;
  /** 0..100 */
  closeness: number;
  impression: string;
  notes: string[];
  /** how they live: hours, food, pet peeves */
  habits: string[];
  /** things still open: promises, deadlines, unanswered questions */
  pending: string[];
  /** shared jokes — the strongest form of "we know each other" */
  jokes: string[];
  /** 0..100: how warm she feels toward them right now */
  warmth: number;
  /** ISO time until which she is still a little annoyed; "" = none */
  grudgeUntil: string;
  /** why she last felt the way she does */
  lastReason: string;
}

export interface PersonPatch {
  closeness?: number;
  impression?: string;
  note?: string;
  habit?: string;
  pending?: string;
  joke?: string;
  warmth?: number;
  grudgeMinutes?: number;
  /** drop a pending item whose text contains this */
  resolve?: string;
  reason?: string;
}

const PERSON_SECTIONS = ["習慣", "進行中", "一起笑過", "筆記"] as const;
type PersonSection = (typeof PERSON_SECTIONS)[number];

interface PersonBody {
  intro: string[];
  sec: Record<PersonSection, string[]>;
}

function emptyPersonBody(): PersonBody {
  return { intro: [], sec: { 習慣: [], 進行中: [], 一起笑過: [], 筆記: [] } };
}

/** Legacy nodes have bare bullets and no headings; those become notes. */
function parsePersonBody(body: string): PersonBody {
  const out = emptyPersonBody();
  let cur: PersonSection | null = null;

  for (const raw of body.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trimEnd();
    const heading = /^##\s+(.+)$/.exec(line);
    if (heading) {
      const name = (heading[1] ?? "").trim();
      cur = (PERSON_SECTIONS as readonly string[]).includes(name)
        ? (name as PersonSection)
        : null;
      continue;
    }
    if (line.startsWith("# ") || line.trim() === "") continue;
    if (line.startsWith("- ")) {
      out.sec[cur ?? "筆記"].push(line.slice(2).trim());
    } else {
      out.intro.push(line.trim());
    }
  }
  return out;
}

function renderPersonBody(b: PersonBody): string {
  const parts: string[] = [];
  if (b.intro.length > 0) parts.push(b.intro.join("\n"));
  for (const s of PERSON_SECTIONS) {
    const items = b.sec[s];
    if (items.length === 0) continue;
    const bullets = items.map((x) => `- ${x}`).join("\n");
    parts.push(`## ${s}\n${bullets}`);
  }
  return parts.join("\n\n");
}

function pushCapped(list: string[], value: string, cap: number): void {
  const clean = value.replace(/\s+/g, " ").trim().slice(0, 120);
  if (clean === "") return;
  if (list.some((x) => x.includes(clean) || clean.includes(x))) return;
  list.push(clean);
  while (list.length > cap) list.shift();
}

export interface MemoryEvent {
  at: Date;
  kind: "heard" | "said";
  text: string;
  where?: string;
  who?: string;
  whoId?: string;
  mood?: string;
  spontaneous?: boolean;
}

export interface MemoryPort {
  /** A volatility-4 block of pointers, or null when nothing is relevant. */
  recall(text: string, now: Date): Block | null;
  /** Append one message to the journal, then make it searchable. */
  log(event: MemoryEvent): void;
  /** Pointers only. */
  search(query: string, limit?: number): Hit[];
  /** Read an actual file window. */
  open(path: string, fromLine?: number, count?: number): string;
  /** One day's journal lines. */
  timeline(date?: Date): string[];
  /** Her own diary entry for a day. */
  diary(date?: Date): string | null;
  /** What she thinks of a person. */
  person(id: string): PersonProfile | null;
  setPerson(id: string, patch: PersonPatch, at: Date): PersonProfile | null;
}

const MAX_POINTERS = 2;
const MAX_BLOCK_CHARS = 260;
const MAX_SNIPPET = 50;

function clip(s: string, n: number): string {
  const flat = s.replace(/\s+/g, " ").trim();
  return [...flat].length <= n ? flat : [...flat].slice(0, n).join("") + "…";
}

function mtimeOf(root: string, rel: string): number {
  try {
    return statSync(join(root, rel)).mtimeMs;
  } catch {
    return 0;
  }
}

/**
 * The vault as a memory port. Retrieval returns pointers with short snippets:
 * cheap enough to inject every turn, deep enough to be useful, and always
 * citable so the model can go read the whole note later.
 */
export class VaultMemory implements MemoryPort {
  private root: string;
  private index: MemoryIndex;

  constructor(root: string) {
    this.root = root;
    this.index = new MemoryIndex();
    for (const rel of listMarkdown(root)) {
      const text = readDoc(root, rel);
      this.index.add({ path: rel, text, mtime: mtimeOf(root, rel) });
    }
  }

  recall(text: string, _now: Date): Block | null {
    const query = text.trim();
    if ([...query].length < 2) return null;

    const hits = this.index.search(query, 8);
    if (hits.length === 0) return null;

    const lines: string[] = ["<memory>", "（可能相關，要細節就用工具讀）"];

    let used = 0;
    for (const hit of hits.slice(0, MAX_POINTERS)) {
      const line = this.renderPointer(hit);
      if (used + line.length > MAX_BLOCK_CHARS) break;
      lines.push(line);
      used += line.length;
    }

    if (lines.length <= 2) return null;
    lines.push("</memory>");
    return { id: "memory", volatility: 4, content: lines.join("\n") };
  }
  search(query: string, limit = 6): Hit[] {
    if (query.trim() === "") return [];
    return this.index.search(query, limit);
  }

  open(path: string, fromLine = 1, count = 40): string {
    try {
      const body = readWindow(this.root, path, fromLine, count);
      if (body === "") return "";
      if (fromLine <= 1) return body;

      // Reading from the middle of a day file loses the date header, which is
      // exactly how she ends up calling today "a few days ago".
      const head = readWindow(this.root, path, 1, 1).trim();
      return head === "" ? body : `${head}\n（第 ${fromLine} 行起）\n${body}`;
    } catch {
      return "";
    }
  }

  timeline(date = new Date()): string[] {
    const abs = journalPath(this.root, date);
    if (!existsSync(abs)) return [];
    const rel = relative(resolve(this.root), abs).replace(/\\/g, "/");

    try {
      return readDoc(this.root, rel)
        .split(/\r?\n/)
        .filter((line) => line.startsWith("- "))
        .slice(-80);
    } catch {
      return [];
    }
  }

  person(id: string): PersonProfile | null {
    const rel = `nodes/people/${id}.md`;
    try {
      const text = readDoc(this.root, rel);
      const parts = text.split("---");
      const fm = parts[1] ?? "";
      const body = parsePersonBody(parts.slice(2).join("---"));
      const field = (key: string): string => {
        const m = new RegExp(`^${key}:\\s*(.+)$`, "m").exec(fm);
        if (!m) return "";
        const v = (m[1] ?? "").trim();
        if (!v.startsWith('"')) return v;
        try {
          return String(JSON.parse(v));
        } catch {
          return v.slice(1, -1);
        }
      };
      const num = (key: string, fallback: number): number => {
        const raw = field(key);
        if (raw === "") return fallback;
        const n = Number(raw);
        return Number.isFinite(n) ? n : fallback;
      };
      return {
        id,
        name: field("name") || id,
        closeness: num("closeness", 50),
        impression: field("impression"),
        notes: body.sec["筆記"].slice(-12),
        habits: body.sec["習慣"].slice(-8),
        pending: body.sec["進行中"].slice(-8),
        jokes: body.sec["一起笑過"].slice(-6),
        warmth: num("warmth", 50),
        grudgeUntil: field("grudge_until"),
        lastReason: field("last_reason"),
      };
    } catch {
      return null;
    }
  }

  setPerson(id: string, patch: PersonPatch, at: Date): PersonProfile | null {
    const rel = `nodes/people/${id}.md`;
    const abs = join(this.root, rel);
    if (!existsSync(abs)) return null;

    const text = readFileSync(abs, "utf8");
    const parts = text.split("---");
    if (parts.length < 3) return null;

    let fm = parts[1] ?? "";
    const bodyRaw = parts.slice(2).join("---");
    const parsed = parsePersonBody(bodyRaw);
    const title = /^#\s+(.+)$/m.exec(bodyRaw)?.[1]?.trim() || "（沒有名字）";

    const put = (key: string, value: string): void => {
      const re = new RegExp(`^${key}:.*$`, "m");
      fm = re.test(fm) ? fm.replace(re, `${key}: ${value}`) : `${fm.trimEnd()}\n${key}: ${value}\n`;
    };

    if (typeof patch.closeness === "number") {
      put("closeness", String(Math.min(100, Math.max(0, Math.round(patch.closeness)))));
    }
    if (typeof patch.warmth === "number") {
      put("warmth", String(Math.min(100, Math.max(0, Math.round(patch.warmth)))));
    }
    if (typeof patch.grudgeMinutes === "number") {
      const until =
        patch.grudgeMinutes <= 0
          ? ""
          : new Date(at.getTime() + patch.grudgeMinutes * 60_000).toISOString();
      put("grudge_until", until === "" ? '""' : until);
    }
    if (typeof patch.reason === "string" && patch.reason.trim() !== "") {
      put("last_reason", JSON.stringify(patch.reason.trim().slice(0, 80)));
    }
    if (typeof patch.impression === "string") {
      put("impression", JSON.stringify(patch.impression.slice(0, 120)));
    }

    if (patch.note && patch.note.trim() !== "") {
      pushCapped(parsed.sec["筆記"], `${at.toISOString().slice(0, 10)} ${patch.note}`, 12);
    }
    if (patch.habit && patch.habit.trim() !== "") {
      pushCapped(parsed.sec["習慣"], patch.habit, 8);
    }
    if (patch.pending && patch.pending.trim() !== "") {
      const dated = /^\d{4}-\d{2}-\d{2}/.test(patch.pending.trim())
        ? patch.pending
        : `${at.toISOString().slice(0, 10)} ${patch.pending}`;
      pushCapped(parsed.sec["進行中"], dated, 8);
    }
    if (patch.joke && patch.joke.trim() !== "") {
      pushCapped(parsed.sec["一起笑過"], patch.joke, 6);
    }
    if (patch.resolve && patch.resolve.trim() !== "") {
      const key = patch.resolve.trim();
      parsed.sec["進行中"] = parsed.sec["進行中"].filter(
        (item) => !item.includes(key) && !key.includes(item),
      );
    }

    const next = `---${fm}---\n\n# ${title}\n\n${renderPersonBody(parsed).trim()}\n`;
    writeFileSync(abs, next, "utf8");
    this.index.add({ path: rel, text: next, mtime: at.getTime() });
    return this.person(id);
  }

  diary(date = new Date()): string | null {
    const day = `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
    try {
      return readDoc(this.root, `diary/${day}.md`);
    } catch {
      return null;
    }
  }

  log(event: MemoryEvent): void {
    const where = event.where ? `（${event.where}）` : "";
    const who = (event.who ?? "").replace(/[|\]]/g, "").replace(/：/g, "").trim();

    const speaker =
      event.kind === "said"
        ? event.spontaneous
          ? "我（忽然）"
          : "我"
        : event.whoId && who !== ""
          ? `[[people/${event.whoId}|${who}]]`
          : who !== ""
            ? who
            : "使用者";

    const body = clip(event.text, event.kind === "heard" ? 400 : 320);
    const abs = journalPath(this.root, event.at);
    const rel = relative(resolve(this.root), abs).replace(/\\/g, "/");
    const hhmm = `${pad2(event.at.getHours())}:${pad2(event.at.getMinutes())}`;

    mkdirSync(dirname(abs), { recursive: true });
    if (!existsSync(abs)) writeFileSync(abs, `# ${dayKey(event.at)}\n\n`, "utf8");

    const lines = readFileSync(abs, "utf8").replace(/\r/g, "").split("\n");
    while (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();

    let changed = 0;

    if (event.kind === "heard") {
      const lastIndex = lines.length - 1;
      const last = lines[lastIndex] ?? "";
      const parsed = parseHeardLine(last);
      const merge =
        parsed !== null &&
        parsed.where === where &&
        parsed.speaker === speaker &&
        minutesBetween(parsed.hhmm, hhmm) <= 15;

      if (merge && lastIndex >= 0) {
        lines[lastIndex] = `${last} ／ ${body}`;
        changed = lastIndex;
      } else {
        lines.push(`- ${hhmm} ${where}${speaker}：${body}`);
        changed = lines.length - 1;
      }
    } else {
      // her reply sits under the group, indented, like a transcript
      lines.push(`　${speaker}：${body}${event.mood ? `（${event.mood}）` : ""}`);
      changed = lines.length - 1;
    }

    writeFileSync(abs, `${lines.join("\n")}\n`, "utf8");
    this.index.appendLine(rel, changed + 1, lines[changed] ?? "", event.at.getTime());

    if (event.kind === "heard" && event.whoId) {
      const jot: Jot = { path: rel, line: changed + 1 };
      this.ensurePerson(event.whoId, who, event.where ?? "", jot, event.at);
    }
  }

  /** A person node, created on first sight. Edges come from journal links. */
  private ensurePerson(id: string, name: string, where: string, jot: Jot, at: Date): void {
    const rel = `nodes/people/${id}.md`;
    const abs = join(this.root, rel);
    if (existsSync(abs)) return;

    try {
      mkdirSync(dirname(abs), { recursive: true });
      const safe = name.replace(/"/g, "'");
      const doc = [
        "---",
        `id: "${id}"`,
        "type: person",
        `name: "${safe}"`,
        `aliases: ["${safe}"]`,
        `first_seen: ${at.toISOString()}`,
        `first_where: "${where.replace(/"/g, "'")}"`,
        "---",
        "",
        `# ${safe || id}`,
        "",
        `第一次出現在 [[${jot.path}#L${jot.line}]]。`,
        "",
        "## 習慣",
        "",
        "## 進行中",
        "",
        "## 一起笑過",
        "",
      ].join("\n");

      writeFileSync(abs, doc, "utf8");
      this.index.add({ path: rel, text: doc, mtime: Date.now() });
    } catch {
      // A failed node must not break logging.
    }
  }
  private renderPointer(hit: Hit): string {
    const snippet = this.snippet(hit.path, hit.line);
    const tail = snippet === "" ? "" : ` → 「${snippet}」`;
    return `- [[${hit.path}#L${hit.line}]]${tail}`;
  }

  private snippet(path: string, line: number): string {
    try {
      return clip(readWindow(this.root, path, line, 1), MAX_SNIPPET);
    } catch {
      return "";
    }
  }
}


