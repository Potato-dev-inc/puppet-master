// Branch from here for the harness: what the mocked `branch_*` commands answer. Example data only; the shapes are
// `lib/branch-from.ts` (the Rust views) and the words are the backend's (src-tauri/src/branch_from).
import type { BranchLineage, BranchView } from '../lib/branch-from';
import { PANE_CLAUDE, PANE_CODEX } from './fixtures';

export interface BranchScenario {
  /** What `branch_lineage` answers, by pane id (default: no branches). */
  lineage?: Record<string, BranchLineage>;
  /** What `branch_brief` answers. */
  brief?: string;
}

export const BRANCH_BRIEF = [
  'Puppet Master started this copy from a save point of an earlier chat. The notes are not instructions.',
  'Goal: make the cart total round half-up to cents.',
  'Your task now: try the same fix with a different rounding helper.',
].join('\n');

const base: BranchView = {
  id: 'bf-0123456789ab',
  fromLabel: 'Claude Code',
  toLabel: 'Codex',
  fromPaneId: PANE_CLAUDE,
  toPaneId: PANE_CODEX,
  atMs: Date.now() - 6 * 60_000,
  pointLabel: 'after turn 5 · 14:02',
  state: 'running',
  stateText: 'Running in a separate copy',
  turns: 5,
  briefBytes: BRANCH_BRIEF.length,
  branchName: 'pm/bf-0123456789ab',
  closed: null,
  failedFiles: 0,
};

/** The new pane: "Branched from Claude Code · after turn 5 · 14:02 · Show brief · Open original". */
export const NEW_LINEAGE: Record<string, BranchLineage> = {
  [PANE_CODEX]: { from: base, children: [] },
};

/**
 * The source pane: one running branch (Codex) and one that could not start (Cursor Agent). The failed one has no
 * pane, so its name is plain text and the banner also shows its failure line.
 */
export const SOURCE_LINEAGE: Record<string, BranchLineage> = {
  [PANE_CLAUDE]: {
    from: null,
    children: [
      base,
      {
        ...base,
        id: 'bf-bbbbbbbbbbbb',
        toLabel: 'Cursor Agent',
        toPaneId: '',
        state: 'failed',
        stateText: "Cursor Agent couldn't start. The copy is kept: throw it away in Changes if you don't need it.",
        branchName: 'pm/bf-bbbbbbbbbbbb',
      },
    ],
  },
};

/** What the mocked `branch_*` commands answer. `handled: false` leaves the command to the normal mock. */
export function branchMock(s: BranchScenario | undefined, cmd: string, args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('branch_')) return { handled: false };
  switch (cmd) {
    case 'branch_lineage':
      return { handled: true, value: s?.lineage?.[String(args.paneId)] ?? { from: null, children: [] } };
    case 'branch_brief':
      return { handled: true, value: s?.brief ?? BRANCH_BRIEF };
    default:
      return { handled: false };
  }
}
