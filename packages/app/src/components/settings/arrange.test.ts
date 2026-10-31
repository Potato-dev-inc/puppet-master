// How the six sections are laid out: group titles, General > Labs, the safety rows, and the links left behind.
import { describe, expect, it } from 'vitest';
import { LABS_GROUP, LABS_INDEX_ID, LAYOUT, arrangeSections, collectLabs, layoutRowIds } from './arrange';
import { BASE_SECTIONS, SECTIONS } from './catalog';
import { SECTION_IDS, groupRows, type RowDef, type SectionDef } from './model';

const section = (id: string) => SECTIONS.find((s) => s.id === id)!;
const rowIn = (sectionId: string, rowId: string) => section(sectionId).rows.find((r) => r.id === rowId);
const baseRow = (id: string) => BASE_SECTIONS.flatMap((s) => s.rows).find((r) => r.id === id)!;

describe('the layout names real rows', () => {
  it('every id in LAYOUT is a row (or the Labs overview), and none is placed twice', () => {
    const real = new Set(BASE_SECTIONS.flatMap((s) => s.rows.map((r) => r.id)));
    real.add(LABS_INDEX_ID);
    const ids = layoutRowIds();
    for (const id of ids) expect(real.has(id), `LAYOUT names "${id}" which is not a row`).toBe(true);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('arranging loses no row and adds only links and the Labs overview', () => {
    const before = BASE_SECTIONS.flatMap((s) => s.rows.map((r) => r.id)).sort();
    const after = SECTIONS.flatMap((s) => s.rows)
      .filter((r) => !r.pointer && r.id !== LABS_INDEX_ID)
      .map((r) => r.id)
      .sort();
    expect(after).toEqual(before);
  });

  it('keeps the six sections, in order, with the same titles', () => {
    expect(SECTIONS.map((s) => s.id)).toEqual([...SECTION_IDS]);
    expect(SECTIONS.map((s) => s.title)).toEqual(BASE_SECTIONS.map((s) => s.title));
  });

  it('does not touch a row: same label, hint and render function as its own definition', () => {
    for (const row of SECTIONS.flatMap((s) => s.rows).filter((r) => !r.pointer && r.id !== LABS_INDEX_ID)) {
      const own = baseRow(row.id);
      expect(row.label).toBe(own.label);
      expect(row.hint).toBe(own.hint);
      expect(row.render).toBe(own.render);
      expect(row.danger).toBe(own.danger);
    }
  });
});

describe('groups inside the sections', () => {
  it('every row has a group title (a destructive row sits in the last list), and a title is not split in two', () => {
    for (const s of SECTIONS) {
      for (const row of s.rows.filter((r) => !r.danger)) expect(row.group, `${s.id}/${row.id}`).toBeTruthy();
      const heads = groupRows(s.rows)
        .filter((g) => !g.danger)
        .map((g) => g.head);
      expect(new Set(heads).size, `${s.id} repeats a group title: ${heads.join(', ')}`).toBe(heads.length);
    }
  });

  it('General reads: Project, Startup, Appearance, Safety, Budgets, Undo and memory, When you are away, Focus Time, Labs, Live Room, Storage, About', () => {
    const heads = groupRows(section('general').rows)
      .filter((g) => !g.danger)
      .map((g) => g.head);
    expect(heads).toEqual(['Project', 'Startup', 'Appearance', 'Safety', 'Budgets', 'Undo and memory', 'When you are away', 'Focus Time', 'Labs', 'Live Room', 'Storage', 'About']);
  });

  it('"Show the welcome" (first-run) sits in General > Appearance, after the Simple mode link', () => {
    const rows = section('general').rows;
    const welcome = rows.find((r) => r.id === 'welcome');
    expect(welcome?.group).toBe('Appearance');
    const appearance = rows.filter((r) => r.group === 'Appearance').map((r) => r.id);
    expect(appearance[appearance.length - 1]).toBe('welcome');
    expect(appearance).toContain('general-simple-mode-link');
  });

  it('Phone starts with Pair a phone, then the paired phones, then the settings, then the public address', () => {
    const heads = groupRows(section('phone').rows).map((g) => g.head);
    expect(heads.filter(Boolean)).toEqual(['Pair a phone', 'Your phones', 'On the phone', 'Public address', 'Telegram (early)']);
    expect(section('phone').rows[0].id).toBe('pairing');
  });

  it('the agent safety switches are in General > Safety, not under the MCP snippets', () => {
    for (const id of ['security-ask-before-control', 'security-agents-merge', 'security-recent-approvals']) {
      expect(rowIn('general', id)?.group, id).toBe('Safety');
      expect(rowIn('bridge', id), `${id} left Bridge`).toBeUndefined();
    }
    const link = section('bridge').rows.find((r) => r.pointer && r.group === 'Safety');
    expect(link?.hint).toContain('General › Safety');
  });

  it('Bridge & MCP starts with the one-step "Add Puppet Master to your agents"', () => {
    expect(section('bridge').rows[0].id).toBe('agent-pack');
  });
});

describe('General > Labs', () => {
  const labsRows = () => section('general').rows.filter((r) => r.group === LABS_GROUP);

  it('collects every early feature in one place, in a fixed order, after the overview', () => {
    expect(labsRows().map((r) => r.id)).toEqual([LABS_INDEX_ID, 'team', 'ask-bar', 'simple-mode', 'live-updates', 'opencode-automation', 'missions', 'assign-issue', 'night-shift', 'red-to-green', 'potato-labs']);
  });

  it('each feature has a short name and one plain line (no second sentence, no raw ids)', () => {
    const features = collectLabs(BASE_SECTIONS);
    expect(features.map((r) => r.id)).toEqual(['team', 'ask-bar', 'simple-mode', 'live-updates', 'opencode-automation', 'missions', 'assign-issue', 'night-shift', 'red-to-green', 'potato-labs']);
    for (const row of features) {
      const { name, about } = row.labs!;
      expect(name.length, row.id).toBeGreaterThan(2);
      expect(about, row.id).toMatch(/[.]$/);
      expect(about.split('. ').length, `${row.id}: one line`).toBe(1);
      expect(about.length, row.id).toBeLessThanOrEqual(90);
      expect(about, row.id).not.toMatch(/Hook Lane|hooks|_/);
    }
  });

  it('the features left their old homes; a link stays where each one used to be', () => {
    expect(rowIn('agents', 'live-updates')).toBeUndefined();
    expect(rowIn('keys', 'opencode-automation')).toBeUndefined();
    const links = SECTIONS.flatMap((s) => s.rows.filter((r) => r.pointer).map((r) => `${s.id}:${r.label}`));
    expect(links).toEqual(expect.arrayContaining(['general:Simple mode', 'agents:Live updates', 'keys:Key switching']));
    for (const link of SECTIONS.flatMap((s) => s.rows).filter((r) => r.pointer && r.label !== 'Agent safety')) {
      expect(link.hint, link.id).toMatch(/General › Labs|under Labs/);
    }
    expect(rowIn('agents', 'agents-live-updates-link')?.hint).toContain('Now in General › Labs.');
    expect(rowIn('general', 'general-simple-mode-link')?.hint).toContain('Now under Labs, further down.');
  });

  it('a feature another branch adds (a row with `labs`, in any section) is collected without touching this file', () => {
    const extra: RowDef = {
      id: 'later-feature',
      label: 'Missions',
      hint: 'Run an agent until a goal is met.',
      labs: { name: 'Missions', about: 'Run an agent until its checks pass.' },
      group: 'Missions',
      render: () => null,
    };
    const base: SectionDef[] = BASE_SECTIONS.map((s) => (s.id === 'phone' ? { ...s, rows: [...s.rows, extra] } : s));
    const arranged = arrangeSections(base);
    const labs = arranged
      .find((s) => s.id === 'general')!
      .rows.filter((r) => r.group === LABS_GROUP)
      .map((r) => r.id);
    expect(labs.slice(-5)).toEqual(['assign-issue', 'night-shift', 'red-to-green', 'potato-labs', 'later-feature']);
    expect(arranged.find((s) => s.id === 'phone')!.rows.some((r) => r.id === 'later-feature')).toBe(false);
  });

  it('a row nobody placed (for example a first-run row) stays in its own section, under its own group, after the named ones', () => {
    const extra: RowDef = { id: 'onboarding', label: 'Welcome tour', hint: 'Show the first-run tour again.', group: 'Welcome', render: () => null };
    const base: SectionDef[] = BASE_SECTIONS.map((s) => (s.id === 'general' ? { ...s, rows: [...s.rows, extra] } : s));
    const rows = arrangeSections(base).find((s) => s.id === 'general')!.rows;
    expect(rows[rows.length - 1].id).toBe('onboarding');
    expect(rows[rows.length - 1].group).toBe('Welcome');
  });
});

describe('the layout table', () => {
  it('covers all seven sections, and Labs lists the `@labs` placeholder once', () => {
    expect(Object.keys(LAYOUT).sort()).toEqual([...SECTION_IDS].sort());
    const placeholders = Object.values(LAYOUT)
      .flatMap((groups) => groups!.flatMap((g) => g.rows))
      .filter((e) => e === '@labs');
    expect(placeholders).toHaveLength(1);
  });
});
