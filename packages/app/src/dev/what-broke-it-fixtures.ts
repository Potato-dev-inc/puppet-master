// What broke the tests? for the harness: what the mocked `wb_*` commands answer. Example data only; the shapes are
// `lib/what-broke-it.ts`, and the words are the backend's. The scenarios use the sheet's set up and running stages.
import type { JobView, Overview } from '../lib/what-broke-it';

export interface WhatBrokeItScenario {
  /** What `wb_overview` answers. Default: `npm test` declared and three save points. */
  overview?: Overview;
}

export const OVERVIEW: Overview = {
  tests: [{ index: 0, name: 'test', command_text: 'npm test', timeout_min: 10, warnings: [] }],
  suggestions: [{ name: 'test', command_text: 'npm test', source: 'package.json' }],
  points: [
    { checkpointId: '31', label: 'Codex · turn 7 · 14:10', atMs: Date.now() - 40 * 60_000 },
    { checkpointId: '28', label: 'Claude Code · turn 5 · 13:42', atMs: Date.now() - 70 * 60_000 },
    { checkpointId: '19', label: 'An agent · turn 2 · 12:05', atMs: Date.now() - 140 * 60_000 },
  ],
  limits: { max_runs: 16, max_minutes: 20 },
  can: true,
  whyNot: null,
  busy: null,
};

/** A search that has just started: the status line of the running stage. */
export const RUNNING_JOB: JobView = {
  id: 'wb-job-0123',
  state: 'running',
  pointNo: 4,
  points: 9,
  run: 3,
  maxRuns: 16,
  elapsedMs: 120_000,
  maxMs: 1_200_000,
  line: 'Trying save point 4 of 9 · run 3 of 16 · 2 min',
  findingId: null,
};

/** What `wb_start` and `wb_status` answer in the dev harness. */
export function whatBrokeItMock(s: WhatBrokeItScenario | undefined, cmd: string, _args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('wb_')) return { handled: false };
  switch (cmd) {
    case 'wb_overview':
      return { handled: true, value: s?.overview ?? OVERVIEW };
    case 'wb_start':
    case 'wb_status':
      return { handled: true, value: RUNNING_JOB };
    case 'wb_stop':
      return { handled: true, value: undefined };
    default:
      return { handled: false };
  }
}
