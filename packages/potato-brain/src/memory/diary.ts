import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import type { Provider } from "../types.ts";
import { journalPath, readDoc } from "./vault.ts";

function dayKey(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function diaryPath(date: Date): string {
  return `diary/${dayKey(date)}.md`;
}

/**
 * One first-person entry per day, generated from the journal. This is both a
 * readable diary and the compaction artifact: tomorrow she can recall the
 * day without replaying every message.
 */
export async function writeDiary(opts: {
  root: string;
  date: Date;
  provider: Provider;
  model?: string;
  maxTokens?: number;
}): Promise<{ path: string; text: string } | null> {
  const abs = journalPath(opts.root, opts.date);
  if (!existsSync(abs)) return null;

  const rel = relative(resolve(opts.root), abs).replace(/\\/g, "/");
  const journal = readDoc(opts.root, rel).slice(-6000);
  const day = dayKey(opts.date);

  const res = await opts.provider.chat({
    messages: [
      {
        role: "system",
        content:
          "你是一隻叫「藍色大肥魚」的藍色胖魚，有點傲嬌，最喜歡白飯。" +
          "一律使用繁體中文（台灣用語），不要簡體字。" + "你要用第一人稱寫今天的日記，300 字以內，口語、短句、自然。" +
          "不要條列、不要標題、不要說「今天」開頭，直接寫。",
      },
      {
        role: "user",
        content: `今天（${day}）的紀錄：\n${journal}\n\n寫今天的日記。只寫你真心在意的事，不要逐句摘要。`,
      },
    ],
    maxTokens: opts.maxTokens ?? 800,
    ...(opts.model ? { model: opts.model } : {}),
  });

  const body = res.text.trim();
  if (body === "") return null;

  const doc = [
    "---",
    `date: ${day}`,
    "type: diary",
    "---",
    "",
    `# ${day}`,
    "",
    body,
    "",
    `> 來源：[[${rel}]]`,
    "",
  ].join("\n");

  const relOut = diaryPath(opts.date);
  const outAbs = join(opts.root, relOut);
  mkdirSync(join(opts.root, "diary"), { recursive: true });
  writeFileSync(outAbs, doc, "utf8");

  return { path: relOut, text: body };
}

