// Collision Alert for the harness: what the mocked `collision_*` commands answer. Example data only; the shapes are
// `lib/collision.ts` (the Rust views) and the words are the backend's (src-tauri/src/collision/copy.rs).
import type { CollisionSettings, CollisionView } from '../lib/collision';
import { PANE_CLAUDE, PANE_CODEX } from './fixtures';

const now = Date.now();
const MIN = 60_000;

export interface CollisionScenario {
  /** What `collision_list` answers (newest first). Default: no alert. */
  alerts?: CollisionView[];
  /** What `collision_settings_get` answers. Default: on. */
  enabled?: boolean;
}

const FOOTER = 'Nothing was paused. You decide.';

const SIDES: CollisionView['sides'] = [
  { paneId: PANE_CLAUDE, label: 'Claude Code', working: false },
  { paneId: PANE_CODEX, label: 'Codex', working: true },
];

/** Both agents changed lines 40-80 of one file, in separate copies. Claude Code has stopped, so it goes first. */
export const CLASH: CollisionView = {
  id: 'ca-0123456789ab',
  level: 'clash',
  relation: 'copies',
  title: 'Claude Code and Codex both changed src/auth.ts',
  detail: 'Lines 40-80 overlap.',
  advice: 'Finish Claude Code first. It has stopped working. Look at its changes and merge them first. Then Codex can build on it.',
  place: 'In separate copies.',
  footer: FOOTER,
  sides: SIDES,
  files: [{ path: 'src/auth.ts', text: 'Lines 40-80' }],
  more: 0,
  firstMs: now - 3 * MIN,
  lastMs: now - 20_000,
};

/** The same file, far apart lines: a quiet row, no phone item, no banner. */
export const QUIET: CollisionView = {
  id: 'ca-ba9876543210',
  level: 'same_file',
  relation: 'copies',
  title: 'Claude Code and Codex both changed src/billing/invoice.ts',
  detail: 'Different places in the file. No clash yet.',
  advice: null,
  place: 'In separate copies.',
  footer: FOOTER,
  sides: SIDES,
  files: [{ path: 'src/billing/invoice.ts', text: 'Different places' }],
  more: 0,
  firstMs: now - 5 * MIN,
  lastMs: now - 40_000,
};

export const ALERTS: CollisionView[] = [CLASH];
export const QUIET_ALERTS: CollisionView[] = [QUIET];

const ignoredBy = new WeakMap<object, Set<string>>();
const NO_SCENARIO = {};

/** `handled: false` leaves the command to the normal mock. `collision_ignore` hides the alert for the rest of the page. */
export function collisionMock(s: CollisionScenario | undefined, cmd: string, args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  if (!cmd.startsWith('collision_')) return { handled: false };
  const key = s ?? NO_SCENARIO;
  const ignored = ignoredBy.get(key) ?? new Set<string>();
  ignoredBy.set(key, ignored);
  switch (cmd) {
    case 'collision_list':
      return { handled: true, value: (s?.alerts ?? []).filter((v) => !ignored.has(v.id)) };
    case 'collision_ignore':
      ignored.add(String(args.id));
      return { handled: true, value: null };
    case 'collision_settings_get':
      return { handled: true, value: { enabled: s?.enabled ?? true } satisfies CollisionSettings };
    case 'collision_settings_set':
      return { handled: true, value: args.settings };
    default:
      return { handled: false };
  }
}
