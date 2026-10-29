// Review Desk harness fixtures: the list `review_desk_list` answers and the answers of the other `review_desk_*`
// commands. Fake data only: no real names, no real chat text, paths as `C:\Users\you\...`.
import type { ReviewDesk, ReviewFiles, ReviewItem } from '../lib/review-desk';

export interface ReviewDeskScenario {
  /** What `review_desk_list` answers (default: REVIEW_DESK_FULL). */
  desk?: ReviewDesk;
}

const files = (total: number, added: number, deleted: number, names: string[]): ReviewFiles => ({
  total,
  added,
  deleted,
  rows: names.map((path, i) => ({ path, status: 'M', added: i === 0 ? added : 0, deleted: i === 0 ? deleted : 0, binary: false })),
});

const ITEM_A: ReviewItem = {
  key: 'copy:mission-m1',
  source: 'mission',
  fingerprint: 'fp-a1',
  title: 'Make the cart total round to cents',
  agentLabel: 'Codex',
  whereLabel: 'Separate copy of main',
  paneId: 'pane-codex-1',
  finishedAtMs: 1_800_000_000_000,
  turns: 3,
  group: null,
  files: files(4, 52, 20, ['src/money.ts', 'test/cart.test.ts', 'src/cart.ts', 'README.md']),
  level: 'high',
  levelWord: 'High risk',
  tier: 1,
  reasons: [{ flag: 'tests_weakened', sentence: 'The tests were weakened: 2 tests were skipped in cart.test.ts' }],
  keep: { kind: 'merge', label: 'Merge…', into: 'main' },
  throw: { kind: 'discard' },
  fixPaneId: 'pane-codex-1',
  fixWhyNot: null,
};

const ITEM_B: ReviewItem = {
  key: 'issue:run-41',
  source: 'issue',
  fingerprint: 'fp-b1',
  title: '#41 Fix login redirect',
  agentLabel: 'Claude Code',
  whereLabel: 'Issue #41',
  paneId: 'pane-claude-2',
  finishedAtMs: 1_800_000_000_000,
  turns: 1,
  group: null,
  files: files(2, 8, 1, ['src/auth/redirect.ts', '.env']),
  level: 'high',
  levelWord: 'High risk',
  tier: 2,
  reasons: [{ flag: 'sensitive', sentence: 'A file may hold a secret: .env' }],
  keep: { kind: 'issue_push', label: 'Push as draft…' },
  throw: { kind: 'issue_throw' },
  fixPaneId: 'pane-claude-2',
  fixWhyNot: null,
};

const ITEM_C: ReviewItem = {
  key: 'folder:pane-claude-1:4-6',
  source: 'folder',
  fingerprint: 'fp-c1',
  title: 'Add CSV export',
  agentLabel: 'Kilo',
  whereLabel: 'In your folder',
  paneId: 'pane-kilo-1',
  finishedAtMs: 1_800_000_000_000,
  turns: 3,
  group: { rootPaneId: 'pane-claude-1', label: 'Builder and helpers' },
  files: files(2, 30, 4, ['src/export/csv.ts', 'src/export/export.test.ts']),
  level: 'check',
  levelWord: 'Check first',
  tier: 4,
  reasons: [{ flag: 'tests_changed', sentence: 'Test files changed: export.test.ts' }],
  keep: { kind: 'mark', label: 'Looks good' },
  throw: { kind: 'restore' },
  fixPaneId: 'pane-kilo-1',
  fixWhyNot: null,
};

const ITEM_D: ReviewItem = {
  key: 'folder:pane-codex-2:1-1',
  source: 'folder',
  fingerprint: 'fp-d1',
  title: 'Rename helper',
  agentLabel: 'Codex',
  whereLabel: 'In your folder',
  paneId: 'pane-codex-2',
  finishedAtMs: 1_800_000_000_000,
  turns: 1,
  group: null,
  files: files(1, 2, 2, ['src/format.ts']),
  level: 'low',
  levelWord: 'Low risk',
  tier: 7,
  reasons: [],
  keep: { kind: 'mark', label: 'Looks good' },
  throw: { kind: 'no', why: 'The agent is still working.' },
  fixPaneId: 'pane-codex-2',
  fixWhyNot: null,
};

const ITEM_E: ReviewItem = {
  key: 'copy:pm-settings-title',
  source: 'copy',
  fingerprint: 'fp-e1',
  title: 'Fix the settings dialog title',
  agentLabel: 'Codex',
  whereLabel: 'Separate copy of main',
  paneId: 'pane-codex-3',
  finishedAtMs: 1_800_000_000_000,
  turns: 2,
  group: null,
  files: files(1, 1, 1, ['src/settings/Dialog.tsx']),
  level: 'low',
  levelWord: 'Low risk',
  tier: 7,
  reasons: [],
  keep: { kind: 'merge', label: 'Merge…', into: 'main' },
  throw: { kind: 'discard' },
  fixPaneId: 'pane-codex-3',
  fixWhyNot: null,
};

/** Five items, riskiest first: two high risk, one check first, two low risk. */
export const REVIEW_DESK_FULL: ReviewDesk = {
  items: [ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E],
  counts: { total: 5, high: 2, check: 1, low: 2 },
  notes: [],
};

/** Nothing finished, and one source that could not be read. */
export const REVIEW_DESK_EMPTY: ReviewDesk = {
  items: [],
  counts: { total: 0, high: 0, check: 0, low: 0 },
  notes: ['Could not read Night Shift. Its tasks are not listed.'],
};

const DEMO_PATCH = [
  'diff --git a/src/money.ts b/src/money.ts',
  '@@ -10,7 +10,7 @@',
  '-export const total = (a: number) => Math.floor(a);',
  '+export const total = (a: number) => Math.round(a * 100) / 100;',
  '+const TOKEN = "[masked]";',
].join('\n');

/** The mocked `review_desk_*` answers. Returns `handled: false` for any other command. */
export function reviewDeskMock(
  s: ReviewDeskScenario | undefined,
  cmd: string,
  args: Record<string, unknown>,
): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('review_desk_')) return { handled: false };
  const desk = s?.desk ?? REVIEW_DESK_FULL;
  const item = (key: unknown): ReviewItem | undefined => desk.items.find((i) => i.key === key) ?? desk.items[0];
  switch (cmd) {
    case 'review_desk_list':
      return { handled: true, value: desk };
    case 'review_desk_count':
      return { handled: true, value: desk.items.length };
    case 'review_desk_detail':
      return { handled: true, value: { item: item(args.key), patch: DEMO_PATCH, patchTruncated: false, extra: [] } };
    case 'review_desk_keep':
      return { handled: true, value: { sentence: 'Marked as looked at.', gone: true } };
    case 'review_desk_throw_away':
      return { handled: true, value: { sentence: 'Thrown away.', gone: true } };
    case 'review_desk_unmark':
      return { handled: true, value: undefined };
    case 'review_desk_fix':
      return { handled: true, value: item(args.key)?.fixPaneId ?? null };
    default:
      return { handled: false };
  }
}
