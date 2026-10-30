// Where every row lives, and in which group. The rows themselves stay in their section files (sections/*.tsx);
// this file is the one place that says how the six sections are laid out for a person:
//   - the group titles and their order inside each section
//   - which rows moved to another section (the agent safety switches live in General > Safety, not under
//     the MCP snippets) and which early features are collected in General > Labs
//   - the links left behind where a row used to be
// A row that is not named here (for example one a new feature adds) keeps its own `group` and is placed after the
// named groups of its section, so adding a row never needs a change here to show up.
import type { ReactNode } from 'react';
import type { RowDef, SectionDef, SectionId, SettingsEnv } from './model';
import { Btn, SettingBlock, SettingRow } from './settings-ui';

/** One entry of a group: a row id, a link to a row that now lives elsewhere, or `@labs` (every early feature). */
export type LayoutEntry = string | { pointerTo: string; label?: string; note?: string };

export interface LayoutGroup {
  group: string;
  rows: LayoutEntry[];
}

export const LABS_GROUP = 'Labs';
/** Row id of the Labs overview (a deep link to General > Labs). */
export const LABS_INDEX_ID = 'labs-index';

/** Early features list in this order; one added later (a new row with `labs`) is listed after them. */
const LABS_ORDER = ['team', 'ask-bar', 'simple-mode', 'live-updates', 'opencode-automation'];

export const LAYOUT: Partial<Record<SectionId, LayoutGroup[]>> = {
  general: [
    { group: 'Project', rows: ['project-folder'] },
    { group: 'Startup', rows: ['start-at-sign-in'] },
    { group: 'Appearance', rows: ['theme', 'animated-tray', { pointerTo: 'simple-mode' }, 'welcome'] },
    {
      group: 'Safety',
      rows: ['security-ask-before-control', 'security-agents-merge', 'confirm-close', 'loop-guard', 'package-check', 'security-recent-approvals'],
    },
    { group: 'Budgets', rows: ['budget-turns', 'budget-minutes'] },
    { group: 'Undo and memory', rows: ['checkpoints', 'checkpoints-keep', 'shared-memory'] },
    { group: 'When you are away', rows: ['away-minutes'] },
    { group: 'Focus Time', rows: ['focus-time'] },
    { group: LABS_GROUP, rows: [LABS_INDEX_ID, '@labs'] },
    { group: 'Live Room', rows: ['live-room'] },
    { group: 'Storage', rows: ['storage', 'chat-search-index', 'bring-old-chats'] },
    { group: 'About', rows: ['version', 'whats-new', 'settings-file', 'diagnostic-file'] },
    { group: 'Uninstall', rows: ['uninstall'] },
  ],
  agents: [
    { group: 'Logins', rows: ['accounts'] },
    { group: 'Ready to use', rows: ['preload', 'helper-tools'] },
    { group: 'Versions', rows: ['versions'] },
    { group: 'Your own agents', rows: ['definitions'] },
    { group: 'Labs', rows: [{ pointerTo: 'live-updates' }] },
  ],
  models: [
    { group: 'Orchestrator', rows: ['orchestrator-backend', 'librarian-indexer'] },
    { group: 'OpenCode', rows: ['opencode-provider', 'opencode-model'] },
    { group: 'Model for the API orchestrator', rows: ['default-provider', 'default-model', 'custom-models'] },
  ],
  keys: [
    { group: 'Provider keys', rows: ['key-anthropic', 'key-openai', 'key-openrouter'] },
    { group: 'OpenCode keys', rows: ['opencode-keys', 'opencode-restart', { pointerTo: 'opencode-automation' }] },
    { group: 'Remove keys', rows: ['remove-all-keys'] },
  ],
  bridge: [
    { group: 'Connect your agents', rows: ['agent-pack', 'toolbelt'] },
    { group: 'Status', rows: ['bridge-endpoint', 'mcp-status'] },
    { group: 'Install', rows: ['mcp-install', 'rust-mcp'] },
    { group: 'Set up by hand', rows: ['snippet-claude-desktop', 'snippet-codex'] },
    {
      group: 'Safety',
      rows: [{ pointerTo: 'security-ask-before-control', label: 'Agent safety', note: 'What agents may do without asking is now in General › Safety.' }],
    },
    { group: 'Uninstall', rows: ['mcp-uninstall'] },
  ],
  phone: [
    { group: 'Pair a phone', rows: ['pairing'] },
    { group: 'Your phones', rows: ['paired-devices', 'revoke-all-devices'] },
    { group: 'On the phone', rows: ['phone-notifications', 'mobile-input-delay', 'mobile-keyboard-resizes', 'mobile-input-visible'] },
    { group: 'Public address', rows: ['tunnel'] },
  ],
};

/** Every row id the layout names (not the `@labs` placeholder or the links). Used to catch stale ids. */
export function layoutRowIds(layout = LAYOUT): string[] {
  const ids: string[] = [];
  for (const groups of Object.values(layout)) {
    for (const g of groups ?? []) for (const entry of g.rows) if (typeof entry === 'string' && entry !== '@labs') ids.push(entry);
  }
  return ids;
}

// ---------- the rows this file adds ----------

function LabsIndex({ env, labs }: { env: SettingsEnv; labs: RowDef[] }) {
  return (
    <SettingBlock label="Early features" description="Each one is off until you turn it on, and may change. Tap one to jump to its switch.">
      <ul className="st-labs-list">
        {labs.map((row) => (
          <li key={row.id}>
            <button type="button" className="st-labs-item" onClick={() => env.openRow?.(row.id)}>
              <span className="st-labs-name">{row.labs?.name}</span>
              <span className="st-labs-about">{row.labs?.about}</span>
            </button>
          </li>
        ))}
      </ul>
    </SettingBlock>
  );
}

/** Where a row went: the target's own name, one line on where it is now, and a button that jumps to it. */
function PointerRow({ target, label, text, env }: { target: RowDef; label: string; text: string; env: SettingsEnv }): ReactNode {
  return (
    <SettingRow label={label} description={text}>
      <Btn onClick={() => env.openRow?.(target.id)}>Show</Btn>
    </SettingRow>
  );
}

// ---------- arrange ----------

/** The early features of every section, in the order they are listed in Labs. */
export function collectLabs(sections: SectionDef[]): RowDef[] {
  const all = sections.flatMap((s) => s.rows).filter((r) => r.labs);
  const rank = (id: string) => {
    const i = LABS_ORDER.indexOf(id);
    return i < 0 ? LABS_ORDER.length : i;
  };
  return all.map((row, index) => ({ row, index })).sort((a, b) => rank(a.row.id) - rank(b.row.id) || a.index - b.index).map((x) => x.row);
}

/**
 * The sections as a person sees them. Pure: it returns new section objects and never changes the rows'
 * own definitions (their ids, labels, hints and render functions), only where they sit and under which group.
 */
export function arrangeSections(base: SectionDef[], layout: Partial<Record<SectionId, LayoutGroup[]>> = LAYOUT): SectionDef[] {
  const byId = new Map<string, { row: RowDef; section: SectionDef }>();
  for (const section of base) for (const row of section.rows) byId.set(row.id, { row, section });

  const labs = collectLabs(base);
  const labsIndex: RowDef | null =
    labs.length === 0
      ? null
      : {
          id: LABS_INDEX_ID,
          label: 'Labs',
          hint: 'Early features that are off until you turn them on.',
          keywords: 'labs early experimental preview beta features',
          render: (env) => <LabsIndex env={env} labs={labs} />,
        };
  if (labsIndex) byId.set(labsIndex.id, { row: labsIndex, section: base[0] });

  const claimed = new Set<string>(layoutRowIds(layout));
  for (const row of labs) claimed.add(row.id);

  return base.map((section) => {
    const groups = layout[section.id];
    if (!groups) return section;
    const rows: RowDef[] = [];
    for (const g of groups) {
      for (const entry of g.rows) {
        if (entry === '@labs') {
          for (const row of labs) rows.push({ ...row, group: g.group, keywords: `${row.keywords ?? ''} labs early` });
          continue;
        }
        if (typeof entry === 'string') {
          const found = byId.get(entry);
          if (found) rows.push({ ...found.row, group: g.group });
          continue;
        }
        const target = byId.get(entry.pointerTo);
        if (!target) continue;
        const destSection = target.row.labs ? 'general' : (base.find((s) => s.rows.some((r) => r.id === target.row.id))?.id ?? section.id);
        const destTitle = base.find((s) => s.id === destSection)?.title ?? 'Settings';
        const destGroup = target.row.labs ? LABS_GROUP : (target.row.group ?? '');
        const where = destSection === section.id ? `Now under ${destGroup}, further down.` : `Now in ${destTitle} › ${destGroup}.`;
        const text = entry.note ?? `${where} ${target.row.labs?.about ?? ''}`.trim();
        const label = entry.label ?? target.row.labs?.name ?? target.row.label;
        rows.push({
          id: `${section.id}-${entry.pointerTo}-link`,
          label,
          hint: text,
          pointer: true,
          group: g.group,
          render: (env) => <PointerRow target={target.row} label={label} text={text} env={env} />,
        });
      }
    }
    // Rows the layout does not name (a new feature's own rows) keep their group, after the named ones.
    for (const row of section.rows) if (!claimed.has(row.id)) rows.push(row);
    return { ...section, rows };
  });
}
