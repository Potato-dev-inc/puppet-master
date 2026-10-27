// The activity line of a turn: consecutive tool rows fold into one line ("Read 6 files · Ran 2 commands"). This file
// groups a turn's steps and writes that line. Pure code.
import type { ToolCallItem, ToolStep, TurnStep } from '../../lib/sessionChatModel';
import { classifyTool, type ToolKind } from '../../lib/chatToolKinds';
import { plural } from './chatFormat';

/** A run of tool rows (one activity line), or one other step (text, a notice, a helper row). */
export type Segment = { kind: 'activity'; id: string; steps: ToolStep[] } | { kind: 'step'; step: TurnStep };

/** A helper row is never folded into a line: it is a row of its own. */
export function segmentSteps(steps: readonly TurnStep[]): Segment[] {
  const out: Segment[] = [];
  for (const step of steps) {
    if (step.kind === 'tool' && !step.call.helper) {
      const last = out[out.length - 1];
      if (last?.kind === 'activity') last.steps.push(step);
      else out.push({ kind: 'activity', id: step.id, steps: [step] });
    } else {
      out.push({ kind: 'step', step });
    }
  }
  return out;
}

export function kindOfCall(call: ToolCallItem): ToolKind {
  return classifyTool({ name: call.name, summary: call.summary, input: call.input, writes: call.writes, helper: call.helper });
}

export interface ActivityCount {
  kind: ToolKind;
  count: number;
}

const FILE_KINDS = new Set<ToolKind>(['read', 'edit', 'write']);

/**
 * How many of each kind, in the order each kind first came. For reads and edits a file counts once however often it
 * was touched; a call with no path counts once.
 */
export function countActivity(steps: readonly ToolStep[]): ActivityCount[] {
  const slots = new Map<ToolKind, { paths: Set<string>; loose: number }>();
  for (const step of steps) {
    const kind = kindOfCall(step.call);
    let slot = slots.get(kind);
    if (!slot) {
      slot = { paths: new Set(), loose: 0 };
      slots.set(kind, slot);
    }
    const paths = (step.call.paths ?? []).filter((path) => typeof path === 'string' && path !== '');
    if (FILE_KINDS.has(kind) && paths.length > 0) for (const path of paths) slot.paths.add(path);
    else slot.loose += 1;
  }
  return [...slots].map(([kind, slot]) => ({ kind, count: slot.paths.size + slot.loose }));
}

const WORDS: Record<ToolKind, (count: number) => string> = {
  read: (n) => `Read ${plural(n, 'file')}`,
  search: (n) => `Searched ${plural(n, 'time')}`,
  edit: (n) => `Edited ${plural(n, 'file')}`,
  write: (n) => `Wrote ${plural(n, 'file')}`,
  run: (n) => `Ran ${plural(n, 'command')}`,
  web: (n) => `Used the web ${plural(n, 'time')}`,
  helper: (n) => `Started ${plural(n, 'helper')}`,
  other: (n) => `Used ${plural(n, 'tool')}`,
};

/** "Read 6 files · Ran 2 commands · Edited 3 files". */
export function activitySummary(steps: readonly ToolStep[]): string {
  return countActivity(steps)
    .map((part) => WORDS[part.kind](part.count))
    .join(' · ');
}

/** The label of the newest running row: its name and summary ("Bash npm test"), or null when none runs. */
export function runningLabel(steps: readonly ToolStep[]): string | null {
  for (let i = steps.length - 1; i >= 0; i--) {
    const step = steps[i]!;
    if (step.status === 'running') return `${step.call.name} ${step.call.summary}`.trim();
  }
  return null;
}

/** Milliseconds from the first row's start to the last row's end, when both times are known; else null. */
export function activitySpanMs(steps: readonly ToolStep[]): number | null {
  if (steps.length === 0) return null;
  const first = steps[0]!;
  const last = steps[steps.length - 1]!;
  const start = Date.parse(first.call.at ?? '');
  const end = Date.parse(last.result?.at ?? last.call.at ?? '');
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return null;
  return end - start;
}
