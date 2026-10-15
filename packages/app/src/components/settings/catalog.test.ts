// The six sections hold every live control of the old 22 tabs, once, under unique ids.
import { describe, expect, it } from 'vitest';
import { LATER_ITEMS, SECTIONS } from './catalog';
import { SECTION_IDS, groupRows, rowMatches, rowScore, searchSections } from './model';

const allRows = SECTIONS.flatMap((s) => s.rows.map((r) => ({ section: s.id, row: r })));

describe('settings catalog', () => {
  it('has exactly the six sections, in order', () => {
    expect(SECTIONS.map((s) => s.id)).toEqual([...SECTION_IDS]);
    expect(SECTIONS.map((s) => s.title)).toEqual(['General', 'Agents', 'Orchestrator & Models', 'Keys', 'Bridge & MCP', 'Phone']);
  });

  it('every row has a unique id, a label and a hint', () => {
    const ids = allRows.map((r) => r.row.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const { section, row } of allRows) {
      expect(row.label.trim(), `${section}/${row.id} label`).not.toBe('');
      expect(row.hint?.trim(), `${section}/${row.id} hint`).toBeTruthy();
    }
  });

  it('every section has rows, and destructive rows are only ever in the last group', () => {
    for (const section of SECTIONS) {
      expect(section.rows.length, section.id).toBeGreaterThan(0);
      const groups = groupRows(section.rows);
      groups.slice(0, -1).forEach((g) => expect(g.danger, `${section.id} danger group not last`).toBe(false));
    }
  });

  it('keeps no planned or placeholder wording in any label or hint', () => {
    for (const { row } of allRows) {
      expect(`${row.label} ${row.hint}`, row.id).not.toMatch(/planned|not implemented|coming soon/i);
    }
  });

  it('the destructive rows are the ones the brief names', () => {
    const danger = allRows.filter((r) => r.row.danger).map((r) => `${r.section}/${r.row.id}`);
    expect(danger.sort()).toEqual(['bridge/mcp-uninstall', 'general/uninstall', 'keys/remove-all-keys', 'phone/revoke-all-devices']);
  });

  it('Coming later lists only what is still to build: Cost controls became Budgets; Team, Security and Plugins ship', () => {
    expect(LATER_ITEMS).toHaveLength(6);
    for (const shipped of ['Cost controls', 'Team workspace', 'Security', 'Plugins']) {
      expect(LATER_ITEMS.map((item) => item.title)).not.toContain(shipped);
    }
    for (const item of LATER_ITEMS) {
      expect(item.title.length).toBeGreaterThan(2);
      expect(item.note.length).toBeGreaterThan(10);
    }
  });
});

describe('row search over the real catalog', () => {
  it('finds a row by a word in its label, hint or keywords, grouped under its section in nav order', () => {
    const hits = searchSections(SECTIONS, 'key');
    expect(hits.map((h) => h.section.id)).toEqual(expect.arrayContaining(['keys', 'phone']));
    const order = hits.map((h) => SECTION_IDS.indexOf(h.section.id));
    expect([...order].sort((a, b) => a - b)).toEqual(order);
  });

  it.each([
    ['checkpoints', 'checkpoints'],
    ['librarian', 'librarian-indexer'],
    ['toolbelt', 'toolbelt'],
    ['qr', 'pairing'],
    ['uninstall', 'uninstall'],
    ['idle timeout', 'preload'],
    ['tunnel', 'tunnel'],
    ['loop', 'loop-guard'],
    ['tray', 'animated-tray'],
    ['away', 'away-minutes'],
  ])('"%s" finds the %s row', (query, id) => {
    const ids = SECTIONS.flatMap((s) => s.rows.filter((r) => rowMatches(r, query)).map((r) => r.id));
    expect(ids).toContain(id);
  });
});

describe('search ranking and coverage', () => {
  const rows = SECTIONS.flatMap((s) => s.rows).filter((r) => !r.pointer);
  const find = (id: string) => rows.find((r) => r.id === id)!;

  it('a hint only matches a whole word: "key" no longer finds "keyboard"', () => {
    expect(rowMatches(find('away-minutes'), 'key')).toBe(false);
    expect(rowMatches(find('away-minutes'), 'keyboard')).toBe(true);
    // the Settings file hint says "keys removed": that is not a row about keys
    expect(rowMatches(find('settings-file'), 'key')).toBe(false);
  });

  it('a label match outranks a keyword match, and a keyword match outranks a hint match', () => {
    const make = (over: Partial<(typeof rows)[number]>) => ({ id: 'x', label: 'Colours', hint: '', keywords: '', render: () => null, ...over });
    const label = make({ label: 'Anthropic API key' });
    const wordStart = make({ label: 'Keychain access' }); // starts with "key" but is not the word
    const keyword = make({ keywords: 'key secret' });
    const hint = make({ hint: 'Choose the key colour.' });
    expect(rowScore(label, 'key')).toBeGreaterThan(rowScore(wordStart, 'key'));
    expect(rowScore(wordStart, 'key')).toBeGreaterThan(rowScore(keyword, 'key'));
    expect(rowScore(keyword, 'key')).toBeGreaterThan(rowScore(hint, 'key'));
    expect(rowScore(hint, 'key')).toBe(1);
    expect(rowScore(make({}), 'key')).toBe(0);
  });

  it('results list the best group of a section first, and keep a group together', () => {
    for (const hit of searchSections(SECTIONS, 'key')) {
      const seen: (string | undefined)[] = [];
      for (const row of hit.rows) if (seen[seen.length - 1] !== row.group) seen.push(row.group);
      expect(new Set(seen).size, `${hit.section.id} groups interleave`).toBe(seen.length);
    }
    const keys = searchSections(SECTIONS, 'key').find((h) => h.section.id === 'keys')!;
    expect(keys.rows[0].group).toBe('Provider keys');
  });

  it('every row is found by its own label, and by every whole word of its hint', () => {
    for (const row of rows) {
      expect(rowMatches(row, row.label), `${row.id} by label`).toBe(true);
      for (const word of (row.hint ?? '').toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean)) {
        expect(rowMatches(row, word), `${row.id} by hint word "${word}"`).toBe(true);
      }
    }
  });

  it('plural and singular both find a row: "keys" finds the API key rows', () => {
    expect(rowMatches(find('key-openai'), 'keys')).toBe(true);
    expect(rowMatches(find('key-openai'), 'openai key')).toBe(true);
  });

  it('a link left where a row moved is never a search hit (the real row is)', () => {
    const everything = searchSections(SECTIONS, 'live updates').flatMap((h) => h.rows);
    expect(everything.filter((r) => r.id === 'live-updates')).toHaveLength(1);
    expect(everything.some((r) => r.pointer)).toBe(false);
  });

  it('Labs: "labs", "early" and each feature name find the right row', () => {
    const hit = (q: string) => searchSections(SECTIONS, q).flatMap((h) => h.rows.map((r) => r.id));
    expect(hit('labs')).toEqual(expect.arrayContaining(['labs-index', 'team', 'ask-bar', 'simple-mode', 'live-updates', 'opencode-automation']));
    expect(hit('hook lane')).toContain('live-updates');
    expect(hit('simple')).toContain('simple-mode');
  });
});

describe('plain words in labels and hints', () => {
  const rows = SECTIONS.flatMap((s) => s.rows);
  const banned = /TOML|vendor\/model|\.md|\.py|hooks?|Hook Lane|worktree|PWA|_[a-z]+|[a-z]+_[a-z]+/;
  it('no raw ids, file names or snake_case on the page, and not the word "Hook Lane"', () => {
    for (const row of rows) expect(`${row.label} ${row.hint}`, row.id).not.toMatch(banned);
  });
  it('every row has a hint, and a hint is one or two short sentences', () => {
    for (const row of rows) {
      expect(row.hint, row.id).toBeTruthy();
      expect((row.hint ?? '').length, `${row.id} hint length`).toBeLessThanOrEqual(260);
    }
  });
});
