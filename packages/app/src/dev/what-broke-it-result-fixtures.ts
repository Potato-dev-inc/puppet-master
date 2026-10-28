// What broke it result card for the harness: example findings, one per result kind, and what the mocked
// `wb_*` result commands answer. Example data only. Shapes are `lib/what-broke-it.ts`; the words are the backend's
// (src-tauri/src/checkpoints/bisect/words.rs). Paths in the output are already masked, as the backend masks them.
import type { DiffView, FindingView, JobView, UndoPlanView } from '../lib/what-broke-it';

const ID = 'wb-0123456789ab';

/** The culprit turn: Codex, turn 7, four files. */
export const CULPRIT: FindingView = {
  id: ID,
  title: 'Turn 7 by Codex broke the tests',
  subtitle: '14:10 · "refactor cart" · 4 files, +52 −20',
  kind: 'culprit',
  suspects: [
    {
      paneKey: 'pane-codex',
      turn: 7,
      agentLabel: 'Codex',
      agentId: 'codex',
      account: null,
      atMs: 1_800_000_000_000,
      sentence: 'refactor cart',
      files: { count: 4, added: 52, deleted: 20 },
    },
  ],
  failing: ['cart.test.ts', 'totals.test.ts'],
  tail: 'not ok 3 - rounds half up\n  expected 12.35, got 12.34\n  at ~\\dev\\cart\\cart.test.ts:14',
  canUndo: true,
  undoneAtMs: null,
};

/** The same culprit after "Undo only this turn": the undo time is set and the button is gone. */
export const CULPRIT_UNDONE: FindingView = { ...CULPRIT, undoneAtMs: 1_800_000_060_000 };

/** Two agents at the same time: no undo, two rows and the plain reason. */
export const SEVERAL: FindingView = {
  id: 'wb-89abcdef0123',
  title: 'Two agents were working at the same time. It was one of these:',
  subtitle: '',
  kind: 'several',
  suspects: [
    { paneKey: 'pane-codex', turn: 6, agentLabel: 'Codex', agentId: 'codex', account: null, atMs: 1_800_000_000_000, sentence: 'refactor cart', files: { count: 2, added: 30, deleted: 4 } },
    { paneKey: 'pane-claude', turn: 5, agentLabel: 'Claude Code', agentId: 'claude_code', account: null, atMs: 1_800_000_000_000, sentence: 'tidy the totals', files: null },
  ],
  failing: ['cart.test.ts'],
  tail: '',
  canUndo: false,
  undoneAtMs: null,
};

/** Broke between two turns: the sentence only. */
export const BETWEEN_TURNS: FindingView = {
  id: 'wb-13579bdf2468',
  title: 'It broke between turns, not during one. Something changed the files between 13:42 and 14:02: you, an install, or a tool.',
  subtitle: '',
  kind: 'between_turns',
  suspects: [],
  failing: [],
  tail: '',
  canUndo: false,
  undoneAtMs: null,
};

/** Out of runs: the range is narrowed, so "Keep looking" shows. */
export const NOT_SURE_RUNS: FindingView = {
  id: 'wb-2468ace13579',
  title: 'Not sure. Ran out of runs.',
  subtitle: 'It was one of turns 5 to 8 (Codex, Claude Code).',
  kind: 'not_sure',
  suspects: [],
  failing: [],
  tail: '',
  canUndo: false,
  undoneAtMs: null,
};

/** Flaky: the same files pass and fail. No "Keep looking". */
export const NOT_SURE_FLAKY: FindingView = {
  ...NOT_SURE_RUNS,
  id: 'wb-2468ace13580',
  title: 'Not sure. The tests pass and fail on the same files (they are flaky).',
  subtitle: '',
};

export const ALWAYS_RED: FindingView = {
  id: 'wb-00ff00ff00ff',
  title: 'The tests were already failing at the oldest save point (11:02). I can\'t look further back.',
  subtitle: '',
  kind: 'always_red',
  suspects: [],
  failing: [],
  tail: '',
  canUndo: false,
  undoneAtMs: null,
};

export const GOOD_WAS_RED: FindingView = {
  id: 'wb-11ee11ee11ee',
  title: 'The tests also fail at the save point you picked (11:02). Pick an earlier one, or choose Find it for me.',
  subtitle: '',
  kind: 'good_was_red',
  suspects: [],
  failing: [],
  tail: '',
  canUndo: false,
  undoneAtMs: null,
};

export const TESTS_PASS: FindingView = {
  id: 'wb-22dd22dd22dd',
  title: 'The tests pass in a clean copy. If they fail in your folder they may need files that save points don\'t keep (installed packages, .env). Add setup steps for the copy in Settings > Copies.',
  subtitle: '',
  kind: 'tests_pass',
  suspects: [],
  failing: [],
  tail: '',
  canUndo: false,
  undoneAtMs: null,
};

/** The sanity gate. The backend words for this kind are not in words.rs yet; this text is a placeholder. */
export const COPY_DIFFERS: FindingView = {
  id: 'wb-33cc33cc33cc',
  title: 'The test copy did not match the save point, so this answer is not trusted.',
  subtitle: '',
  kind: 'copy_differs',
  suspects: [],
  failing: [],
  tail: '',
  canUndo: false,
  undoneAtMs: null,
};

export const FAILED: FindingView = {
  id: 'wb-44ee44ee44ee',
  title: 'npm was not found on this PC.',
  subtitle: '',
  kind: 'failed',
  suspects: [],
  failing: [],
  tail: '',
  canUndo: false,
  undoneAtMs: null,
};

/** A dry run with nothing in the way. */
export const UNDO_PLAN_OK: UndoPlanView = {
  writes: ['cart.ts', 'cart.test.ts'],
  removes: [],
  conflicts: [],
  blockedBy: null,
};

/** A later turn touched the same lines in cart.ts. */
export const UNDO_PLAN_CONFLICT: UndoPlanView = {
  writes: [],
  removes: [],
  conflicts: [{ path: 'cart.ts', why: 'Later work changed the same lines' }],
  blockedBy: null,
};

/** The file list of the culprit turn. */
export const CULPRIT_DIFF: DiffView = {
  files: [
    { path: 'src/cart.ts', status: 'M', added: 40, deleted: 12, binary: false },
    { path: 'src/cart.test.ts', status: 'M', added: 12, deleted: 8, binary: false },
    { path: 'src/totals.ts', status: 'A', added: 0, deleted: 0, binary: true },
  ],
  patch: null,
  truncated: false,
};

/** The patch of one file, as text. */
export const CART_PATCH: DiffView = {
  files: [],
  patch: '@@ -1,3 +1,3 @@\n-export const rate = 0.12;\n+export const rate = 0.125;\n',
  truncated: false,
};

/** The job a recheck answers: one run, already done. */
const RECHECK_JOB: JobView = {
  id: 'wbj-000000000001',
  state: 'done',
  pointNo: 1,
  points: 1,
  run: 1,
  maxRuns: 1,
  elapsedMs: 900,
  maxMs: 60_000,
  line: 'Checked the folder now.',
  findingId: null,
};
const RECHECK_FINDING_ID = 'wb-55ee55ee55ee';

export interface WhatBrokeItResultScenario {
  /** What `wb_finding` (and `wb_findings`) answer. */
  finding?: FindingView;
  /** What `wb_undo_plan` answers. Default: no conflicts. */
  undoPlan?: UndoPlanView;
  /** What `wb_undo` answers when the plan is clean. */
  undoRefusedMessage?: string;
  /** What `wb_recheck` finishes with (its finding is `recheckFinding`). */
  recheckFinding?: FindingView;
}

/** The mocked `wb_*` result commands. Returns `handled: false` for every other command. */
export function whatBrokeItResultMock(
  s: WhatBrokeItResultScenario | undefined,
  cmd: string,
  args: Record<string, unknown>,
): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('wb_')) return { handled: false };
  const finding = s?.finding ?? CULPRIT;
  switch (cmd) {
    case 'wb_finding':
      if (args.findingId === RECHECK_FINDING_ID) return { handled: true, value: s?.recheckFinding ?? finding };
      return { handled: true, value: finding };
    case 'wb_findings':
      return { handled: true, value: [finding] };
    case 'wb_diff':
      return { handled: true, value: args.path ? CART_PATCH : CULPRIT_DIFF };
    case 'wb_undo_plan':
      return { handled: true, value: s?.undoPlan ?? UNDO_PLAN_OK };
    case 'wb_undo':
      if (s?.undoRefusedMessage) throw new Error(s.undoRefusedMessage);
      return { handled: true, value: { undoneAtMs: 1_800_000_060_000, written: 2, removed: 0 } };
    case 'wb_recheck':
      return { handled: true, value: { ...RECHECK_JOB, findingId: s?.recheckFinding ? RECHECK_FINDING_ID : null } };
    case 'wb_status':
      return { handled: true, value: { ...RECHECK_JOB, id: String(args.jobId) } };
    default:
      return { handled: false };
  }
}
