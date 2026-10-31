import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export interface MigrationResult {
  files: number;
  converted: number;
}

const LEGACY = /^(\d{2}:\d{2})\s*使用者(（[^）]*）)?\s*：([\s\S]*)$/;
const NAME = /^([^：\s，。]{1,12})：([\s\S]*)$/;
const HEARD = /^- (\d{2}:\d{2}) (（[^）]*）)?(.+?)：([\s\S]*)$/;

function minutesBetween(a: string, b: string): number {
  const [ah, am] = a.split(":").map(Number);
  const [bh, bm] = b.split(":").map(Number);
  const diff = Math.abs((bh ?? 0) * 60 + (bm ?? 0) - ((ah ?? 0) * 60 + (am ?? 0)));
  return Math.min(diff, 1440 - diff);
}

/** Same speaker, same room, within 15 minutes -> one line with ／ between. */
function mergeLines(lines: string[]): string[] {
  const out: string[] = [];

  for (const line of lines) {
    const next = HEARD.exec(line);
    const prev = out.length > 0 ? HEARD.exec(out[out.length - 1] ?? "") : null;

    if (
      next &&
      prev &&
      (prev[2] ?? "") === (next[2] ?? "") &&
      prev[3] === next[3] &&
      minutesBetween(prev[1] ?? "", next[1] ?? "") <= 15
    ) {
      out[out.length - 1] = `${out[out.length - 1]} ／ ${next[4]}`;
      continue;
    }

    out.push(line);
  }

  return out;
}

function migrateText(text: string): { text: string; converted: number } {
  const out: string[] = [];
  let converted = 0;

  for (const line of text.replace(/\r/g, "").split("\n")) {
    if (!line.startsWith("- ")) {
      out.push(line);
      continue;
    }

    const match = LEGACY.exec(line.slice(2));
    if (!match) {
      out.push(line);
      continue;
    }

    const hhmm = match[1] ?? "";
    const where = match[2] ?? "";
    const content = match[3] ?? "";

    const cut = content.lastIndexOf("｜我：");
    const heard = cut >= 0 ? content.slice(0, cut) : content;
    const said = cut >= 0 ? content.slice(cut + 3) : "";

    // Only trust the "NAME：text" prefix when the bot actually replied —
    // that shape came from the old speaker prefix, not from the message.
    const named = cut >= 0 ? NAME.exec(heard) : null;
    const speaker = named ? (named[1] ?? "") : "(未記錄)";
    const body = (named ? (named[2] ?? heard) : heard).trim();

    out.push(`- ${hhmm} ${where}${speaker}：${body}`);
    if (said.trim() !== "") out.push(`　我：${said.trim()}`);
    converted++;
  }

  return { text: mergeLines(out).join("\n"), converted };
}

export function migrateJournals(root: string): MigrationResult {
  let files = 0;
  let converted = 0;

  const walk = (dir: string): void => {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir)) {
      const abs = join(dir, name);
      const stat = statSync(abs);
      if (stat.isDirectory()) {
        walk(abs);
        continue;
      }
      if (!name.endsWith(".md")) continue;

      const text = readFileSync(abs, "utf8");
      if (!text.includes("使用者")) continue;

      const result = migrateText(text);
      if (result.text !== text) {
        writeFileSync(`${abs}.bak`, text, "utf8");
        writeFileSync(abs, result.text, "utf8");
        files++;
        converted += result.converted;
      }
    }
  };

  walk(join(root, "journal"));
  return { files, converted };
}

