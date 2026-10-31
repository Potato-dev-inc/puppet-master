import { mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Msg, Provider } from "../types.ts";

export interface CheckpointResult {
  rel: string;
  text: string;
  /** the index actually used; may be higher than the requested one */
  index: number;
}

function safeScope(scope: string): string {
  const cleaned = scope.replace(/[^\w.-]+/g, "_").replace(/^_+|_+$/g, "");
  return cleaned.slice(0, 40) || "main";
}

/**
 * Turn a chunk of a conversation into a compact checkpoint. This is the
 * "epoch boundary": after it, the prompt changes once and then stays stable
 * again, instead of silently dropping detail when the hot window rolls.
 */
export async function writeCheckpoint(opts: {
  root: string;
  scope: string;
  index: number;
  messages: Msg[];
  provider: Provider;
  maxTokens?: number;
}): Promise<CheckpointResult | null> {
  const transcript = opts.messages
    .filter((m) => m.role === "user" || m.role === "assistant")
    .map((m) => `${m.role === "user" ? "對方" : "我"}：${m.content.slice(0, 360)}`)
    .join("\n")
    .slice(-6000);

  if (transcript.trim() === "") return null;

  const res = await opts.provider.chat({
    messages: [
      {
        role: "system",
        content:
          "把以下對話壓成重點。只保留：出現的人、發生的事、做過的決定、還沒完成的事、情緒轉折。" +
          "最多 5 條，每條一行，用繁體中文。不要客套、不要開場白、不要編造沒發生的事。",
      },
      { role: "user", content: transcript },
    ],
    maxTokens: opts.maxTokens ?? 500,
  });

  const text = res.text.trim();
  if (text === "") return null;

  const scope = safeScope(opts.scope);
  const dir = join(opts.root, "sessions", scope);
  mkdirSync(dir, { recursive: true });

  // The in-memory counter resets on restart; never overwrite an older run.
  let index = Math.max(1, Math.trunc(opts.index));
  try {
    for (const name of readdirSync(dir)) {
      const match = /^cp-(\d+)\.md$/.exec(name);
      if (match) index = Math.max(index, Number(match[1]) + 1);
    }
  } catch {
    // empty or unreadable directory: the requested index is fine
  }

  const rel = `sessions/${scope}/cp-${String(index).padStart(3, "0")}.md`;
  writeFileSync(
    join(opts.root, rel),
    `---\nscope: "${scope}"\nindex: ${index}\ntype: checkpoint\n---\n\n${text}\n`,
    "utf8",
  );

  return { rel, text, index };
}
