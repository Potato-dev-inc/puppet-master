// The timeline of a clip: four scenes (title, replay, result, end card) and the captions shown over the replay.
// Pure code with no DOM. The drawing reads the scenes and captions made here; the captions use the same activity
// wording as the chat ("Read 6 files · Ran 2 commands"), so no agent is named.
import { activitySummary } from '../components/chat/chatActivity';
import type { ClipPlan, Step } from './clipTypes';
import type { ToolCallItem, ToolResultItem, ToolStep } from './sessionChatModel';

/** A clip is never longer than this. Every timeline is built inside it. */
export const CLIP_MAX_MS = 30_000;
export const TITLE_MS = 3_000;
export const RESULT_MS = 5_000;
export const END_CARD_MS = 2_500;
export const REPLAY_MIN_MS = 4_000;
export const REPLAY_MAX_MS = 18_000;
/** Each caption is on screen at least this long. */
export const CAPTION_MIN_MS = 3_000;
export const CAPTION_MAX_COUNT = 4;
export const CAPTION_MAX_WORDS = 12;
export const REQUEST_MAX_CHARS = 140;

/** One part of the clip. Times are on the clip's own timeline (from 0). */
export type Scene =
  | { kind: 'title'; startMs: number; endMs: number; agent: string; request: string }
  | { kind: 'replay'; startMs: number; endMs: number; mode: 'terminal' | 'activity' }
  | { kind: 'result'; startMs: number; endMs: number }
  | { kind: 'end'; startMs: number; endMs: number; agent: string };

/**
 * Text shown over the clip. `caption` is the line at the bottom; `badge` is the "Sped up 40x" mark that stays on
 * the whole replay (it does not count in the four captions).
 */
export interface Caption {
  startMs: number;
  endMs: number;
  text: string;
  role: 'caption' | 'badge';
}

export interface Timeline {
  scenes: Scene[];
  captions: Caption[];
  totalMs: number;
}

/**
 * Lay out the clip: title (3 s), replay (4 to 18 s, or none), result (5 s), end card (2.5 s). The longest clip is
 * 28.5 s. `requestOverride` replaces only the request shown on the title; an empty one keeps the plan's request.
 */
export function buildTimeline(plan: ClipPlan, requestOverride?: string): Timeline {
  const scenes: Scene[] = [];
  const captions: Caption[] = [];
  const request = titleRequest(plan.request, requestOverride);

  scenes.push({ kind: 'title', startMs: 0, endMs: TITLE_MS, agent: plan.agent, request });
  let at = TITLE_MS;

  const replay = plan.replay;
  if (replay.kind !== 'none') {
    const length = replayLength(replay.lengthMs);
    scenes.push({ kind: 'replay', startMs: at, endMs: at + length, mode: replay.kind });
    captions.push(...stepCaptions(plan.steps, at, length));
    if (replay.kind === 'terminal' && Number.isFinite(replay.speedX) && replay.speedX > 1) {
      captions.push({ startMs: at, endMs: at + length, text: `Sped up ${Math.round(replay.speedX)}x`, role: 'badge' });
    }
    at += length;
  }

  scenes.push({ kind: 'result', startMs: at, endMs: at + RESULT_MS });
  at += RESULT_MS;
  scenes.push({ kind: 'end', startMs: at, endMs: at + END_CARD_MS, agent: plan.agent });
  at += END_CARD_MS;

  return { scenes, captions, totalMs: at };
}

/**
 * The caption for a group of steps: the activity wording ("Read 6 files · Ran 2 commands"), plus "· 1 failed" when a
 * command failed. Cut to at most 12 words by dropping whole parts from the end, so a failure is kept.
 */
export function captionFor(steps: readonly Step[]): string {
  if (steps.length === 0) return '';
  const parts = activitySummary(steps.map(toToolStep))
    .split(' · ')
    .filter((part) => part !== '');
  const failed = steps.filter((step) => step.ok === false).length;
  const tail = failed > 0 ? [`${failed} failed`] : [];
  const line = () => [...parts, ...tail].join(' · ');
  while (parts.length > 0 && wordCount(line()) > CAPTION_MAX_WORDS) parts.pop();
  return cutWords(line(), CAPTION_MAX_WORDS);
}

/**
 * The steps are put into at most 4 equal parts of the replay, each at least 3 s long. A part with steps becomes one
 * caption shown for that part, so captions never overlap and never run past the replay.
 */
function stepCaptions(steps: readonly Step[], startMs: number, lengthMs: number): Caption[] {
  const slots = Math.min(CAPTION_MAX_COUNT, Math.floor(lengthMs / CAPTION_MIN_MS));
  const groups: Step[][] = Array.from({ length: slots }, () => []);
  for (const step of steps) {
    const at = clamp(step.atMs, 0, lengthMs - 1);
    groups[Math.min(slots - 1, Math.floor((at * slots) / lengthMs))]!.push(step);
  }
  const out: Caption[] = [];
  groups.forEach((group, index) => {
    const text = captionFor(group);
    if (text === '') return;
    out.push({
      startMs: startMs + Math.floor((index * lengthMs) / slots),
      endMs: startMs + Math.floor(((index + 1) * lengthMs) / slots),
      text,
      role: 'caption',
    });
  });
  return out;
}

/** The replay length, kept in its bounds. A value that is not a number counts as the shortest. */
function replayLength(lengthMs: number): number {
  return Math.round(clamp(lengthMs, REPLAY_MIN_MS, REPLAY_MAX_MS));
}

/** The request on the title: the override when it has text, else the plan's request. One line, 140 characters. */
function titleRequest(planRequest: string, override: string | undefined): string {
  const typed = (override ?? '').replace(/\s+/g, ' ').trim();
  const text = typed !== '' ? typed : planRequest.replace(/\s+/g, ' ').trim();
  return Array.from(text).slice(0, REQUEST_MAX_CHARS).join('');
}

/** A step as the chat's activity code reads it (a tool row with a call and, when known, a result). */
function toToolStep(step: Step, index: number): ToolStep {
  const id = `clip-${index}`;
  const call: ToolCallItem = {
    kind: 'toolCall',
    id,
    at: null,
    callId: id,
    name: step.name,
    summary: step.summary,
    paths: step.paths,
    writes: step.writes,
    helper: step.helper,
  };
  const result: ToolResultItem | null =
    step.ok === null ? null : { kind: 'toolResult', id: `${id}-result`, at: null, callId: id, ok: step.ok, output: '' };
  return { kind: 'tool', id, call, result, status: step.ok === false ? 'error' : 'ok' };
}

function wordCount(text: string): number {
  return text.split(/\s+/).filter((word) => word !== '').length;
}

function cutWords(text: string, max: number): string {
  const words = text.split(/\s+/).filter((word) => word !== '');
  return words.length <= max ? text : words.slice(0, max).join(' ');
}

function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, value));
}
