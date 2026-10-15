// Missions (Preview): example missions the mocked `mission_*` commands answer in the harness.
// Example data only; the words are the engine's (src-tauri/src/missions/view.rs).
import type { DetectedCheck, MissionView, TimelineRow } from '../lib/missions';
import { PANE_CLAUDE, PANE_CODEX } from './fixtures';

const now = Date.now();
const min = 60_000;

export interface MissionScenario {
  /** The Labs switch (`missions_enabled`). */
  enabled?: boolean;
  /** What `mission_list` answers. */
  list?: MissionView[];
  /** What `mission_detect` answers. */
  detect?: DetectedCheck[];
}

export const DETECTED: DetectedCheck[] = [
  { name: 'Tests', command: ['npm', 'test'], source: 'package.json', on: true },
  { name: 'Type check', command: ['npx', 'tsc', '--noEmit'], source: 'tsconfig.json', on: true },
  { name: 'Lint', command: ['npm', 'run', 'lint'], source: 'package.json', on: false },
];

const turn = (n: number, agent: string, counts: string, delta: string, text: string, ago: number, extra: Partial<TimelineRow> = {}): TimelineRow => ({
  kind: 'turn',
  glyph: 'v',
  turn: n,
  agent,
  counts,
  delta,
  text,
  action: 'show_diff',
  at_ms: now - ago * min,
  checks: [
    { name: 'Tests', passed: false, text: `${counts} pass` },
    { name: 'Type check', passed: true, text: 'passes' },
  ],
  ...extra,
});

const event = (glyph: TimelineRow['glyph'], text: string, ago: number, action?: TimelineRow['action']): TimelineRow => ({
  kind: 'event',
  glyph,
  text,
  at_ms: now - ago * min,
  action,
});

const base: MissionView = {
  id: 'm1',
  handle: 'mission-m1',
  preview: true,
  project: 'C:\\Users\\dev\\work\\shop',
  project_name: 'shop',
  objective: 'Make the cart tests pass and round totals to cents.',
  title: 'Make the cart tests pass and round totals to cents.',
  state: 'working',
  state_text: 'Turn 7 · 9 of 12 pass',
  glyph: '~',
  pause_requested: false,
  turn: 7,
  start: '5 of 12',
  now: '9 of 12',
  passed: 9,
  total: 12,
  budget: { turns_used: 7, turns: 30, minutes_used: 41, minutes: 120, text: '7 of 30 turns · 41 of 120 min' },
  agents: ['Claude Code', 'Codex'],
  agents_line: 'Claude Code → Codex',
  current_agent: 'Codex',
  current_run: 'run-codex-1',
  current_pane: PANE_CODEX,
  copy: { path: 'C:\\Users\\dev\\work\\shop\\.puppet-master\\worktrees\\mission-m1', branch: 'pm/mission-m1', base: 'main', base_commit: '3f2a1c0', closed: false },
  checks: [
    { name: 'Tests', passed: false, text: 'npm test' },
    { name: 'Type check', passed: true, text: 'npx tsc --noEmit' },
  ],
  may_change_tests: false,
  rollback: true,
  test_change_lines: [],
  test_changes_text: 'none',
  timeline: [
    { ...turn(7, 'Codex', '9 of 12', '+2', 'working…', 1), glyph: '~', action: 'open_pane', pane_id: PANE_CODEX, checks: [] },
    turn(6, 'Codex', '7 of 12', '−2', "went back to turn 5's files", 6),
    event('v', "Tests got worse (9 of 12 → 7 of 12). Went back to turn 5's files.", 6.5),
    turn(5, 'Codex', '9 of 12', '+3', '', 12),
    event('v', 'Handed to Codex · Claude Code made no progress', 18, 'show_brief'),
    turn(4, 'Claude Code', '6 of 12', '0', '', 22),
    turn(3, 'Claude Code', '6 of 12', '0', '', 27),
    turn(2, 'Claude Code', '6 of 12', '+1', '', 31),
    turn(1, 'Claude Code', '5 of 12', '0', '', 36),
    event(':', 'Started · 5 of 12 pass at the start · separate copy from main at 3f2a1c0', 41),
  ],
  started_by: 'you',
  work_text: '7 turns · Claude Code 4, Codex 3 · 1 hand-off · 1 time went back',
  created_at_ms: now - 41 * min,
  updated_at_ms: now - min,
  rev: 40,
};

export const MISSION_WORKING: MissionView = base;

export const MISSION_TAMPER: MissionView = {
  ...base,
  id: 'm2',
  handle: 'mission-m2',
  state: 'needs_you',
  needs: 'tamper',
  card_title: 'The checks pass, but Codex changed the tests',
  sentence: 'The checks pass, but the tests were changed',
  state_text: 'Needs you · Tests were changed',
  glyph: '+',
  now: '12 of 12',
  passed: 12,
  total: 12,
  turn: 8,
  test_change_lines: ['Added .skip to "handles 0.005" in cart.test.ts', 'Removed 2 assertions in cart.test.ts'],
  test_changes_text: 'Added .skip to "handles 0.005" in cart.test.ts, Removed 2 assertions in cart.test.ts',
  timeline: [
    { ...turn(8, 'Codex', '12 of 12', '+3', 'changed the tests', 1), glyph: '+' },
    ...base.timeline.slice(1),
  ],
};

export const MISSION_DONE: MissionView = {
  ...base,
  id: 'm3',
  handle: 'mission-m3',
  state: 'green',
  state_text: 'Done · 12 of 12 pass · ready to review',
  sentence: 'Done · 12 of 12 pass · ready to review',
  glyph: 'v',
  now: '12 of 12',
  passed: 12,
  total: 12,
  turn: 9,
  checks: [
    { name: 'Tests', passed: true, text: 'npm test' },
    { name: 'Type check', passed: true, text: 'npx tsc --noEmit' },
  ],
  current_run: undefined,
  work_text: '9 turns · Claude Code 4, Codex 5 · 1 hand-off · 1 time went back',
  budget: { turns_used: 9, turns: 30, minutes_used: 52, minutes: 120, text: '9 of 30 turns · 52 of 120 min' },
  timeline: [
    { ...turn(9, 'Codex', '12 of 12', '+3', '', 2), checks: [{ name: 'Tests', passed: true, text: '12 of 12 pass' }, { name: 'Type check', passed: true, text: 'passes' }] },
    event('v', 'All 12 pass', 1.9),
    turn(8, 'Codex', '9 of 12', '0', '', 7),
    ...base.timeline.slice(1),
  ],
  ended_at_ms: now - 2 * min,
};

export const MISSION_STUCK: MissionView = {
  ...base,
  id: 'm4',
  handle: 'mission-m4',
  objective: 'The importer reads every CSV in fixtures/.',
  title: 'The importer reads every CSV in fixtures/.',
  project_name: 'billing-service',
  state: 'needs_you',
  needs: 'stuck',
  card_title: "The mission isn't getting anywhere",
  sentence: 'No progress after turn 9',
  state_text: 'Needs you · No progress after turn 9',
  glyph: '+',
  current_pane: PANE_CLAUDE,
  current_agent: 'Claude Code',
};

/** `handled: false` leaves the command to the normal mock. */
export function missionMock(s: MissionScenario | undefined, cmd: string, args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('mission_')) return { handled: false };
  const list = s?.list ?? [];
  const find = (id: unknown) => list.find((m) => m.id === id) ?? list[0];
  switch (cmd) {
    case 'mission_list':
      return { handled: true, value: { missions: list } };
    case 'mission_get':
      return { handled: true, value: { ...find(args.id), notes: '- (Claude Code, turn 2) Switched money to integer cents. Next: half-up rounding in total().\n- (Codex, turn 5) Rounding fixed; 3 tests left on discounts.' } };
    case 'mission_detect':
      return { handled: true, value: { checks: s?.detect ?? DETECTED } };
    case 'mission_start':
      return { handled: true, value: { ...base, state: 'planning', state_text: 'Getting ready · setting up the copy', timeline: [] } };
    default:
      return { handled: true, value: find(args.id) };
  }
}
