// @vitest-environment node
// Every shortcut in lib/shortcuts.ts must still have its handler in the source. A grep-style check: the file
// must exist and contain the text the entry names. If a handler is renamed or removed, update the entry too.
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { SHORTCUT_GROUPS, SHORTCUTS, formatKey, shortcutKeys, type ShortcutEntry } from './shortcuts';

// packages/app (this file is packages/app/src/lib).
const appRoot = fileURLToPath(new URL('../../', import.meta.url));

describe('keyboard shortcuts list', () => {
  it.each(SHORTCUTS.flatMap((s) => s.handlers.map((h) => [s.id, h.file, h.text] as const)))(
    '%s: %s has the handler text %s',
    (_id, file, text) => {
      const path = `${appRoot}${file}`;
      expect(existsSync(path), `missing file ${file}`).toBe(true);
      expect(readFileSync(path, 'utf8')).toContain(text);
    },
  );

  it('every entry has an id, a label, keys and at least one handler', () => {
    for (const s of SHORTCUTS) {
      expect(s.label.trim(), s.id).not.toBe('');
      expect(s.keys.length, s.id).toBeGreaterThan(0);
      expect(s.handlers.length, s.id).toBeGreaterThan(0);
    }
  });

  it('ids are unique and every group is one the sheet shows', () => {
    const ids = SHORTCUTS.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const s of SHORTCUTS) expect(SHORTCUT_GROUPS, s.id).toContain(s.group);
  });

  it('shows every group the sheet has at least one shortcut in', () => {
    for (const group of SHORTCUT_GROUPS) {
      expect(SHORTCUTS.some((s) => s.group === group), group).toBe(true);
    }
  });
});

describe('formatKey', () => {
  it('keeps Ctrl on Windows and Linux', () => {
    expect(formatKey('Ctrl+Shift+F', false)).toBe('Ctrl+Shift+F');
    expect(formatKey('Ctrl+\\', false)).toBe('Ctrl+\\');
  });

  it('shows Ctrl as ⌘, Shift as ⇧ and Alt as ⌥ on a Mac', () => {
    expect(formatKey('Ctrl+Shift+F', true)).toBe('⌘⇧F');
    expect(formatKey('Ctrl+\\', true)).toBe('⌘\\');
    expect(formatKey('Ctrl+Alt+Shift+P', true)).toBe('⌘⌥⇧P');
    expect(formatKey('Enter', true)).toBe('Enter');
  });
});

describe('shortcutKeys', () => {
  const entry = (extra: Partial<ShortcutEntry>): ShortcutEntry => ({
    id: 'x',
    group: 'Panes',
    label: 'x',
    keys: ['Ctrl+C'],
    handlers: [],
    ...extra,
  });

  it('uses the macKeys override on a Mac only', () => {
    const e = entry({ keys: ['Ctrl+C'], macKeys: ['⌃C'] });
    expect(shortcutKeys(e, false)).toEqual(['Ctrl+C']);
    expect(shortcutKeys(e, true)).toEqual(['⌃C']);
  });

  it('shows each alternative as its own key', () => {
    expect(shortcutKeys(entry({ keys: ['?', 'Ctrl+/'] }), false)).toEqual(['?', 'Ctrl+/']);
  });
});
