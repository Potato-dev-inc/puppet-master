import { readFileSync, existsSync, mkdirSync, appendFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Block } from "../types.ts";
import type { MoodState } from "./mood.ts";
import { initialMood } from "./mood.ts";
import { resolvePersonaDir } from "./packs.ts";
import { emptySelf } from "./self.ts";
import type { SelfState } from "./self.ts";

export interface Persona {
  blocks: Block[];
  /** the join point for the tool schema block, kept out of the file for now */
  coreHash: string;
}

/**
 * Persona is files on disk, editable by a human, hashed into the prefix.
 * Static blocks only: nothing here may change during a conversation.
 */
export function loadPersona(dir: string, pack?: string): Block[] {
  const base = resolvePersonaDir(dir, pack);
  const tag = (pack ?? "").trim() === "" ? "default" : (pack as string).trim();

  const files: Array<{ path: string; id: string }> = [
    { path: join(base, "core.md"), id: `persona:core:${tag}` },
    { path: join(base, "rules.md"), id: `persona:rules:${tag}` },
    // someone else's prompt goes here, unedited; precedence is in its header
    { path: join(base, "pasted.md"), id: `persona:pasted:${tag}` },
    // protocol is shared by every pack, so packs only need a core.md
    { path: join(dir, "protocol.md"), id: "persona:protocol" },
  ];

  const blocks: Block[] = [];
  for (const f of files) {
    if (!existsSync(f.path)) continue;
    const raw = readFileSync(f.path, "utf8").trim();
    if (raw === "") continue;
    // imported DSH packs still carry `{{model}}` / `{{cwd}}` placeholders
    const content = raw
      .replaceAll("{{model}}", process.env.FISHD_MODEL || "deepseek")
      .replaceAll("{{cwd}}", process.cwd());
    blocks.push({ id: f.id, volatility: 0, content });
  }

  if (blocks.length === 0) {
    blocks.push({
      id: "persona:fallback",
      volatility: 0,
      content: "你是一隻藍色的大肥魚。講話短短的，偶爾傲嬌。",
    });
  }
  return blocks;
}

function ensureDir(dir: string): void {
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
}

export function moodPath(stateDir: string): string {
  return join(stateDir, "mood.json");
}

export function loadMood(stateDir: string, now = new Date()): MoodState {
  const p = moodPath(stateDir);
  if (!existsSync(p)) return initialMood(now);
  try {
    const raw = JSON.parse(readFileSync(p, "utf8")) as Partial<MoodState>;
    const base = initialMood(now);
    return {
      mood: raw.mood ?? base.mood,
      intensity: raw.intensity ?? base.intensity,
      energy: typeof raw.energy === "number" ? raw.energy : base.energy,
      hunger: typeof raw.hunger === "number" ? raw.hunger : base.hunger,
      lastTick: raw.lastTick ?? base.lastTick,
      lastMoodChange: raw.lastMoodChange ?? base.lastMoodChange,
      overrideTurns: typeof raw.overrideTurns === "number" ? raw.overrideTurns : 0,
    };
  } catch {
    return initialMood(now);
  }
}

export function saveMood(stateDir: string, mood: MoodState): void {
  ensureDir(stateDir);
  writeFileSync(moodPath(stateDir), JSON.stringify(mood, null, 2) + "\n", "utf8");
}

/**
 * One fish, many rooms: every session reads and writes the same mood file,
 * so a channel and a DM never disagree about how she feels.
 */
export function moodStore(stateDir: string): {
  load: () => MoodState;
  save: (mood: MoodState) => void;
} {
  return {
    load: () => loadMood(stateDir),
    save: (mood) => saveMood(stateDir, mood),
  };
}

export function selfPath(stateDir: string): string {
  return join(stateDir, "self.json");
}

export function loadSelf(stateDir: string): SelfState {
  const p = selfPath(stateDir);
  if (!existsSync(p)) return emptySelf();
  try {
    const raw = JSON.parse(readFileSync(p, "utf8")) as Partial<SelfState>;
    const base = emptySelf();
    return {
      boredom: typeof raw.boredom === "number" ? raw.boredom : base.boredom,
      slacking: raw.slacking === true,
      task: raw.task ?? null,
      plan: raw.plan ? { ...raw.plan, deferred: raw.plan.deferred ?? 0 } : null,
      pending: raw.pending ?? null,
    };
  } catch {
    return emptySelf();
  }
}

export function saveSelf(stateDir: string, self: SelfState): void {
  ensureDir(stateDir);
  writeFileSync(selfPath(stateDir), JSON.stringify(self, null, 2) + "\n", "utf8");
}

export function selfStore(stateDir: string): {
  load: () => SelfState;
  save: (self: SelfState) => void;
} {
  return {
    load: () => loadSelf(stateDir),
    save: (self) => saveSelf(stateDir, self),
  };
}

/** Append-only log of everything she changes about herself. */
export function appendSelfLog(stateDir: string, line: string, now = new Date()): void {
  ensureDir(stateDir);
  const day = now.toISOString().slice(0, 10);
  const p = join(stateDir, "self-log.md");
  if (!existsSync(p)) writeFileSync(p, "# self log\n\n", "utf8");
  appendFileSync(p, `- ${day} ${line}\n`, "utf8");
}

/** Append-only: the mood log is her memory of how she felt, never rewritten. */
export function appendMoodLog(stateDir: string, line: string, now = new Date()): void {
  ensureDir(stateDir);
  const day = now.toISOString().slice(0, 10);
  const p = join(stateDir, "mood-log.md");
  if (!existsSync(p)) writeFileSync(p, "# mood log\n\n", "utf8");
  appendFileSync(p, `- ${day} ${line}\n`, "utf8");
}
