// The six sections hold every live control of the old 22 tabs, once, under unique ids.
import { describe, expect, it } from 'vitest';
import { LATER_ITEMS, SECTIONS } from './catalog';
import { SECTION_IDS, groupRows, rowMatches, searchSections } from './model';

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

  it('Coming later lists the ten old placeholder tabs with honest notes', () => {
    expect(LATER_ITEMS).toHaveLength(10);
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
