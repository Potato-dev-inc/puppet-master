import { MOODS } from "./mood.ts";
import type { Intensity, Mood } from "./mood.ts";
import { isFeel } from "./kaomoji.ts";
import type { Feel } from "./kaomoji.ts";
import { rand01 } from "../core/hash.ts";

export const TONES = ["softcute", "playful", "healing", "energetic", "whisper"] as const;
export type Tone = (typeof TONES)[number];

export interface Expression {
  mood?: Mood;
  intensity?: Intensity;
  why?: string;
  feel?: Feel;
  segments?: 1 | 2 | 3;
  tone?: Tone;
  react?: string;
  /** a quiet inner thought, shown occasionally as subtext */
  whisper?: string;
}

const MARKER = "::state";

function isMood(x: unknown): x is Mood {
  return typeof x === "string" && (MOODS as readonly string[]).includes(x);
}

function isTone(x: unknown): x is Tone {
  return typeof x === "string" && (TONES as readonly string[]).includes(x);
}

function isIntensity(x: unknown): x is Intensity {
  return x === 1 || x === 2 || x === 3;
}

/**
 * Split the reply from its delivery instructions.
 * The state block is optional; most turns should not have one.
 */
export function parseExpression(raw: string): { text: string; expression: Expression } {
  const idx = raw.lastIndexOf(MARKER);
  if (idx === -1) return { text: raw.trim(), expression: {} };

  const text = raw.slice(0, idx).trim();
  let body = raw.slice(idx + MARKER.length).trim();
  if (body.startsWith("```")) {
    body = body.replace(/^```(?:json)?/i, "").replace(/```$/, "").trim();
  }

  const open = body.indexOf("{");
  const close = body.lastIndexOf("}");
  if (open === -1 || close <= open) return { text, expression: {} };

  let parsed: unknown;
  try {
    parsed = JSON.parse(body.slice(open, close + 1));
  } catch {
    return { text, expression: {} };
  }
  if (!parsed || typeof parsed !== "object") return { text, expression: {} };

  const o = parsed as Record<string, unknown>;
  const expression: Expression = {};

  if (isMood(o.mood)) expression.mood = o.mood;
  if (isIntensity(o.intensity)) expression.intensity = o.intensity;
  if (typeof o.why === "string") expression.why = o.why.slice(0, 120);
  if (isFeel(o.feel)) expression.feel = o.feel;
  if (o.segments === 1 || o.segments === 2 || o.segments === 3) expression.segments = o.segments;
  if (isTone(o.tone)) expression.tone = o.tone;
  if (typeof o.react === "string" && o.react.length <= 8) expression.react = o.react;
  if (typeof o.whisper === "string") {
    const w = o.whisper.trim();
    if (w !== "") expression.whisper = w.slice(0, 80);
  }

  return { text, expression };
}

/**
 * Split into 1..3 chat bubbles at sentence boundaries.
 * Never cuts a sentence in half.
 */
export function splitMessage(text: string, n: 1 | 2 | 3 = 1): string[] {
  const trimmed = text.trim();
  if (n <= 1 || trimmed === "") return [trimmed];

  const parts = trimmed
    .split(/(?<=[。！？!?…\n])/)
    .map((s) => s.trim())
    .filter(Boolean);

  if (parts.length <= 1) return [trimmed];

  const groups: string[] = [];
  const per = Math.ceil(parts.length / n);
  for (let i = 0; i < parts.length; i += per) {
    groups.push(parts.slice(i, i + per).join(""));
  }
  return groups.slice(0, 3);
}

export interface VoicePolicy {
  enabled: boolean;
  probability: number;
  maxChars: number;
}

/**
 * The 50% lottery lives in code, not in the prompt, and is seeded from the
 * message id so it is reproducible and testable.
 */
export function shouldVoice(
  text: string,
  seed: number,
  policy: VoicePolicy,
  hasEmotion: boolean,
): boolean {
  if (!policy.enabled) return false;
  if (!hasEmotion) return false;
  if ([...text].length > policy.maxChars) return false;
  return rand01(seed) < policy.probability;
}

/** Inner thoughts should be rare; otherwise they stop feeling like thoughts. */
export function shouldWhisper(seed: number, probability: number): boolean {
  if (probability <= 0) return false;
  return rand01(seed ^ 0x9e3779b9) < probability;
}

/** Mood drives prosody, so the model doesn't have to remember the mapping. */
export function toneForMood(mood: Mood): Tone {
  switch (mood) {
    case "sleepy":
      return "whisper";
    case "genki":
      return "energetic";
    case "tsundere":
      return "playful";
    case "hungry":
      return "softcute";
  }
}
