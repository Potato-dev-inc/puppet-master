import type { Block } from "../types.ts";

export const MOODS = ["sleepy", "hungry", "genki", "tsundere"] as const;
export type Mood = (typeof MOODS)[number];
export type Intensity = 1 | 2 | 3;

export interface MoodState {
  mood: Mood;
  intensity: Intensity;
  /** 0..100 */
  energy: number;
  /** 0..100 */
  hunger: number;
  /** ISO timestamp of the last physics tick */
  lastTick: string;
  /** ISO timestamp of the last mood change */
  lastMoodChange: string;
  /** turns left on a model/programmatic override; 0 = derived */
  overrideTurns: number;
}

export interface MoodPhysics {
  /** local hour the fish sleeps from (inclusive) */
  sleepStart: number;
  /** local hour the fish wakes (exclusive) */
  sleepEnd: number;
  energyPerSleepHour: number;
  energyPerAwakeHour: number;
  hungerPerSleepHour: number;
  hungerPerAwakeHour: number;
  energyPerMessage: number;
  /** clamp absurd gaps (machine was off for a month) */
  maxGapHours: number;
}

export const DEFAULT_PHYSICS: MoodPhysics = {
  sleepStart: 1,
  sleepEnd: 7,
  energyPerSleepHour: 1.6,
  energyPerAwakeHour: -2.4,
  hungerPerSleepHour: 1.5,
  hungerPerAwakeHour: 4.0,
  energyPerMessage: 5,
  maxGapHours: 72,
};

export interface NudgeInput {
  mood: Mood;
  intensity?: Intensity;
  why?: string;
}

export interface NudgeResult {
  state: MoodState;
  accepted: boolean;
  reason?: string;
  log?: string;
}

export interface MoodContext {
  closeness?: "stranger" | "friend" | "close";
  gapHours?: number;
  serious?: boolean;
}

function clamp(n: number, lo = 0, hi = 100): number {
  return Math.min(hi, Math.max(lo, n));
}

function validDate(d: Date): boolean {
  return Number.isFinite(d.getTime());
}

export function initialMood(now: Date, energy = 62, hunger = 30): MoodState {
  const iso = now.toISOString();
  return {
    mood: "genki",
    intensity: 2,
    energy,
    hunger,
    lastTick: iso,
    lastMoodChange: iso,
    overrideTurns: 0,
  };
}

/**
 * Advance energy/hunger by wall-clock time, then re-derive the mood.
 * Pure: the caller supplies `now`. This runs on every turn, so a bot that
 * was offline for three days still feels like three days passed.
 */
export function tick(
  s: MoodState,
  now: Date,
  physics: MoodPhysics = DEFAULT_PHYSICS,
): MoodState {
  const last = new Date(s.lastTick);
  if (!validDate(last) || !validDate(now)) return s;

  const rawHours = (now.getTime() - last.getTime()) / 3_600_000;
  if (rawHours <= 0) return s;

  const hours = Math.min(rawHours, physics.maxGapHours);
  const slept = sleepHoursBetween(last, now, hours, physics);
  const awake = hours - slept;

  const next: MoodState = {
    ...s,
    energy: clamp(s.energy + slept * physics.energyPerSleepHour + awake * physics.energyPerAwakeHour),
    hunger: clamp(s.hunger + slept * physics.hungerPerSleepHour + awake * physics.hungerPerAwakeHour),
    lastTick: now.toISOString(),
  };

  return derive(next, now);
}

/** Count how many of `hours` fall inside the sleep window, sampled at 15 min. */
function sleepHoursBetween(
  from: Date,
  to: Date,
  hours: number,
  physics: MoodPhysics,
): number {
  const start = from.getTime();
  const end = Math.min(to.getTime(), start + hours * 3_600_000);
  const span = end - start;
  if (span <= 0) return 0;

  const step = 15 * 60 * 1000;
  let asleep = 0;
  let total = 0;
  for (let t = start; t < end; t += step) {
    const d = new Date(t);
    const h = d.getHours() + d.getMinutes() / 60;
    total += step;
    if (h >= physics.sleepStart && h < physics.sleepEnd) asleep += step;
  }
  return total === 0 ? 0 : (asleep / total) * hours;
}

function scores(s: MoodState, now: Date): Record<Mood, number> {
  const h = now.getHours() + now.getMinutes() / 60;

  const sleepy =
    (s.energy < 35 ? ((35 - s.energy) / 35) * 2.2 : 0) +
    (h >= 22 || h < 6 ? 1.1 : 0) +
    (h >= 23 || h < 5 ? 0.5 : 0);

  const hungry =
    (s.hunger > 45 ? ((s.hunger - 45) / 55) * 2.4 : 0) +
    (h >= 11.5 && h < 13.5 ? 0.9 : 0) +
    (h >= 17.5 && h < 19.5 ? 0.6 : 0);

  const genki =
    (s.energy > 50 ? ((s.energy - 50) / 50) * 1.8 : 0) +
    (h >= 6 && h < 11 ? 0.7 : 0) +
    (h >= 18 && h < 22 ? 0.6 : 0);

  const tsundere =
    1.0 - (s.energy > 85 || s.energy < 25 ? 1.2 : 0) - (s.hunger > 80 ? 0.6 : 0);

  return { sleepy, hungry, genki, tsundere };
}

function intensityFor(score: number): Intensity {
  if (score >= 2.2) return 3;
  if (score >= 1.4) return 2;
  return 1;
}

/** Re-derive mood unless a nudge override is still counting down. */
export function derive(s: MoodState, now: Date): MoodState {
  if (s.overrideTurns > 0) return s;

  const sc = scores(s, now);
  let mood: Mood = s.mood;
  let best = sc[mood];

  for (const m of MOODS) {
    if (m === mood) continue;
    // hysteresis: only switch on a clear win
    if (sc[m] > best + 0.25) {
      mood = m;
      best = sc[m];
    }
  }

  if (mood === s.mood) {
    const intensity = intensityFor(best);
    return intensity === s.intensity ? s : { ...s, intensity };
  }

  return {
    ...s,
    mood,
    intensity: intensityFor(best),
    lastMoodChange: now.toISOString(),
  };
}

/**
 * The only way the model can move the mood. Clamped, one per conversation,
 * suppressed entirely when the other person is upset or talking business.
 */
export function nudge(
  s: MoodState,
  input: NudgeInput,
  now: Date,
  opts: { nudgedThisSession: boolean; serious?: boolean },
): NudgeResult {
  if (opts.serious) {
    return { state: s, accepted: false, reason: "serious-mode" };
  }
  if (opts.nudgedThisSession) {
    return { state: s, accepted: false, reason: "already-nudged" };
  }

  const intensity = input.intensity ?? s.intensity;
  if (input.mood === s.mood && intensity === s.intensity) {
    return { state: s, accepted: false, reason: "unchanged" };
  }

  const state: MoodState = {
    ...s,
    mood: input.mood,
    intensity,
    lastMoodChange: now.toISOString(),
    overrideTurns: 3,
  };

  const hh = String(now.getHours()).padStart(2, "0");
  const mm = String(now.getMinutes()).padStart(2, "0");
  const why = (input.why ?? "").replace(/\s+/g, " ").slice(0, 80);
  const log = `${hh}:${mm} ${input.mood}+${intensity} ← ${why || "(no reason)"}`;

  return { state, accepted: true, log };
}

/** Each user message wakes her up a little. */
export function onMessage(s: MoodState, physics: MoodPhysics = DEFAULT_PHYSICS): MoodState {
  return { ...s, energy: clamp(s.energy + physics.energyPerMessage) };
}

/** Count down an active override; the loop calls this once per assistant turn. */
export function advanceTurn(s: MoodState): MoodState {
  if (s.overrideTurns <= 0) return s;
  const overrideTurns = s.overrideTurns - 1;
  return { ...s, overrideTurns };
}

function timeBand(hour: number): string {
  if (hour < 5) return "凌晨";
  if (hour < 7) return "清晨";
  if (hour < 11) return "上午";
  if (hour < 13.5) return "中午";
  if (hour < 17.5) return "下午";
  if (hour < 21) return "傍晚";
  return "深夜";
}

const EFFECT: Record<Mood, string> = {
  sleepy: "回覆變短、常省略主詞，不太主動開新話題",
  hungry: "肚子有點空，容易把話題帶到吃的",
  genki: "語速偏快、句子變長，會主動開話題",
  tsundere: "嘴上挑剔、心裡在意，會假裝不在乎",
};

const CLOSENESS: Record<NonNullable<MoodContext["closeness"]>, string> = {
  stranger: "對這位還不太熟，收斂一點",
  friend: "對這位是熟人",
  close: "對這位很熟，可以多黏一點",
};

/**
 * Render the mood as an effect, never as numbers.
 * The model needs the consequence, not "energy: 74".
 */
export function render(s: MoodState, now: Date, ctx: MoodContext = {}): string {
  if (ctx.serious) {
    return "（對方可能在難過或在談正事，可愛元素全部收起來，先認真接住。）";
  }

  const strengthen = s.intensity === 3 ? "（很明顯）" : s.intensity === 1 ? "（淡淡的）" : "";
  const parts: string[] = [`${EFFECT[s.mood]}${strengthen}`, timeBand(now.getHours())];
  if (ctx.closeness) parts.push(CLOSENESS[ctx.closeness]);
  if (ctx.gapHours && ctx.gapHours >= 20) {
    const days = Math.floor(ctx.gapHours / 24);
    parts.push(days >= 2 ? `你們大概 ${days} 天沒見了` : "對方隔了一陣子才出現");
  }

  return `（現在的你：${parts.join("。")}。）`;
}

/** The tip block. Volatility 4: it goes at the very end, never in the prefix. */
export function moodBlock(
  s: MoodState,
  now: Date,
  ctx: MoodContext = {},
): Block {
  return { id: "tip:mood", volatility: 4, content: render(s, now, ctx) };
}
