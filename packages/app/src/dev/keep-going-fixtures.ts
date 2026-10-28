// Keep going for the harness: what the mocked `keep_going_*` commands answer. Example data only; the shapes are
// `lib/keep-going.ts` (the Rust views) and the words are the backend's (src-tauri/src/handoff/keep_going).
import type { HandoverView, KeepGoingLineage, KeepGoingOffer, KeepGoingSettings } from '../lib/keep-going';
import { PANE_CLAUDE, PANE_CODEX } from './fixtures';

const now = Date.now();
const MIN = 60_000;

export interface KeepGoingScenario {
  /** What `keep_going_targets` answers: the picker rows. */
  targets?: KeepGoingOffer[];
  /** What `keep_going_lineage` answers, by pane id (default: no hand-over). */
  lineage?: Record<string, KeepGoingLineage>;
  /** What `keep_going_brief` answers. */
  brief?: string;
}

export const TARGETS: KeepGoingOffer[] = [
  { target: { agent_id: 'codex' }, label: 'Codex', ok: true, whyNot: null },
  { target: { agent_id: 'cursor_agent' }, label: 'Cursor Agent', ok: false, whyNot: 'Not installed' },
  { target: { agent_id: 'opencode' }, label: 'OpenCode', ok: false, whyNot: 'Out of usage until 16:40' },
];

export const BRIEF = [
  'Continue the task of the previous agent (Claude Code), which ran out of usage.',
  'Goal: make the cart total round half-up to cents.',
  'Done so far: money is now integer cents; 2 of 5 tests pass.',
  'Files changed: src/cart.ts (+48 -7), src/cart.test.ts (+12 -0).',
  'Next: fix rounding in total(), then run the tests.',
  'Ask the person before anything risky. Do not answer permission questions yourself.',
].join('\n');

const base: HandoverView = {
  id: 'kg-0123456789ab',
  fromLabel: 'Claude Code',
  toLabel: 'Codex',
  fromPaneId: PANE_CLAUDE,
  toPaneId: PANE_CODEX,
  atMs: now - 6 * MIN,
  state: 'running',
  stateText: 'Continued on Codex',
  turns: 14,
  briefBytes: BRIEF.length,
  backAtMs: now + 25 * MIN,
  reason: 'usage_limit',
};

/** The hand-over seen from both panes: the old one reads "Continued on", the new one "Continued from". */
export const RUNNING_LINEAGE: Record<string, KeepGoingLineage> = {
  [PANE_CLAUDE]: { from: null, to: base },
  [PANE_CODEX]: { from: base, to: null },
};

/** Codex did not start: the old pane says so and points to the card. */
export const FAILED_LINEAGE: Record<string, KeepGoingLineage> = {
  [PANE_CLAUDE]: { from: null, to: { ...base, state: 'failed', stateText: "Codex couldn't start." } },
};

export const SETTINGS: KeepGoingSettings = { order: [{ agent_id: 'codex' }], ask_back: true };

/** `handled: false` leaves the command to the normal mock. */
export function keepGoingMock(s: KeepGoingScenario | undefined, cmd: string, args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('keep_going_')) return { handled: false };
  switch (cmd) {
    case 'keep_going_targets':
      return { handled: true, value: s?.targets ?? TARGETS };
    case 'keep_going_lineage':
      return { handled: true, value: s?.lineage?.[String(args.paneId)] ?? { from: null, to: null } };
    case 'keep_going_brief':
      return { handled: true, value: s?.brief ?? BRIEF };
    case 'keep_going_continue':
      return { handled: true, value: { ...base, state: 'starting', stateText: 'Moving to Codex' } };
    case 'keep_going_settings_get':
      return { handled: true, value: SETTINGS };
    case 'keep_going_settings_set':
      return { handled: true, value: args.settings };
    default:
      return { handled: false };
  }
}
