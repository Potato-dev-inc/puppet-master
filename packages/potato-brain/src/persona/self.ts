import type { Mood } from "./mood.ts";

/**
 * Everything the fish can change about herself. Mood/energy/hunger live in
 * mood.json; this is the rest of her inner life.
 */
export interface SelfState {
  /** 0..100, rises when nobody talks to her */
  boredom: number;
  slacking: boolean;
  /** what she is doing right now, in her own words */
  task: { what: string; since: string } | null;
  /** when she next wants to speak up, and about what */
  plan: { at: string; topic: string; why: string; deferred: number } | null;
  /** something she wanted to say but never got to say */
  pending: string | null;
}

export interface SelfPatch {
  hunger?: number;
  energy?: number;
  mood?: Mood;
  intensity?: 1 | 2 | 3;
  boredom?: number;
  slacking?: boolean;
  task?: string | null;
  planMinutes?: number;
  planTopic?: string;
  planWhy?: string;
  clearPlan?: boolean;
  pending?: string | null;
  /** push the current plan forward; used when it is not a good time */
  planDeferMinutes?: number;
}

export function emptySelf(): SelfState {
  return { boredom: 20, slacking: false, task: null, plan: null, pending: null };
}

const clamp = (n: number) => Math.min(100, Math.max(0, n));

/** Boredom climbs while idle, drops when someone talks to her. */
export function tickSelf(s: SelfState, now: Date, lastInteraction: number | null): SelfState {
  if (lastInteraction === null) return s;

  const minutes = Math.max(0, (now.getTime() - lastInteraction) / 60_000);
  const delta = minutes < 5 ? -12 : minutes * 0.35;
  return { ...s, boredom: clamp(s.boredom + delta) };
}

export function applySelf(
  s: SelfState,
  patch: SelfPatch,
  now: Date,
): { state: SelfState; log: string[] } {
  const next: SelfState = { ...s };
  const log: string[] = [];

  if (typeof patch.boredom === "number") {
    next.boredom = clamp(patch.boredom);
    log.push(`boredom=${Math.round(next.boredom)}`);
  }

  if (typeof patch.slacking === "boolean") {
    next.slacking = patch.slacking;
    log.push(patch.slacking ? "摸魚" : "不摸魚了");
  }

  if (patch.task !== undefined) {
    if (patch.task === null || patch.task.trim() === "") {
      next.task = null;
      log.push("放下手邊的事");
    } else {
      next.task = { what: patch.task.trim().slice(0, 120), since: now.toISOString() };
      log.push(`開始：${next.task.what}`);
    }
  }

  if (patch.pending !== undefined) {
    const text = (patch.pending ?? "").trim().slice(0, 120);
    if (text === "") {
      next.pending = null;
      log.push("放下沒說出口的話");
    } else {
      next.pending = text;
      log.push(`先記著：${text}`);
    }
  }

  if (patch.clearPlan) {
    next.plan = null;
    log.push("取消計畫");
  } else if (typeof patch.planDeferMinutes === "number" && next.plan) {
    const minutes = Math.min(2880, Math.max(1, patch.planDeferMinutes));
    next.plan = {
      ...next.plan,
      at: new Date(now.getTime() + minutes * 60_000).toISOString(),
      deferred: next.plan.deferred + 1,
    };
    log.push(`往後延 ${minutes} 分`);
  } else if (typeof patch.planMinutes === "number" && patch.planMinutes > 0) {
    const topic = (patch.planTopic ?? "").trim().slice(0, 80);
    if (topic !== "") {
      const minutes = Math.min(2880, Math.max(1, patch.planMinutes));
      next.plan = {
        at: new Date(now.getTime() + minutes * 60_000).toISOString(),
        topic,
        why: (patch.planWhy ?? "").trim().slice(0, 80),
        deferred: 0,
      };
      log.push(`計畫 ${minutes} 分後聊「${topic}」`);
    }
  }

  return { state: next, log };
}

export function planDue(s: SelfState, now: Date): boolean {
  if (!s.plan) return false;
  const at = Date.parse(s.plan.at);
  return Number.isFinite(at) && at <= now.getTime();
}

/** One compact line for the prompt tail. */
export function renderSelf(s: SelfState, now: Date): string {
  const parts: string[] = [];

  if (s.slacking) parts.push("你正在摸魚");
  if (s.boredom >= 60) parts.push("有點無聊");
  else if (s.boredom <= 10) parts.push("現在挺有精神");
  if (s.task) parts.push(`手頭的事：${s.task.what}`);
  if (s.plan) {
    const at = Date.parse(s.plan.at);
    if (Number.isFinite(at)) {
      const minutes = Math.round((at - now.getTime()) / 60_000);
      const held = s.plan.deferred > 0 ? "（已經忍過一次）" : "";
      parts.push(
        minutes > 0
          ? `你打算 ${minutes} 分鐘後聊「${s.plan.topic}」${held}`
          : `你本來想聊「${s.plan.topic}」${held}`,
      );
    }
  }
  if (s.pending) parts.push(`有句話還沒跟他說：${s.pending}`);

  return parts.length === 0 ? "" : `（你自己：${parts.join("；")}）`;
}
