// The generic screen reader on the first screen of a brand-new Claude Code (a real capture, v2.1.312) and on the idle
// input boxes of several agents. The same captures are fixtures of the Rust tests.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { KEY_DOWN, KEY_UP, keysForOption, parseScreenQuestion } from './screen-question';

const fixture = (path: string) => readFileSync(resolve(__dirname, '../../src-tauri/src', path), 'utf8');

/** A text-style picker: a check mark after the pointer, a dotted banner above it, a code preview under it. */
const THEME = fixture('tui/claude/fixtures/first_run_theme_2_1_312.txt');
const LOGIN_METHOD = fixture('tui/claude/fixtures/first_run_login_method_2_1_312.txt');
const API_KEY = fixture('tui/claude/fixtures/first_run_api_key_2_1_312.txt');

describe('a menu whose pointer row carries a mark', () => {
  it('reads the theme picker as seven options, Dark mode selected, with no check mark in the labels', () => {
    const q = parseScreenQuestion(THEME)!;
    expect(q.options.map((o) => o.label)).toEqual([
      'Auto (match terminal)',
      'Dark mode',
      'Light mode',
      'Dark mode (colorblind-friendly)',
      'Light mode (colorblind-friendly)',
      'Dark mode (ANSI colors only)',
      'Light mode (ANSI colors only)',
    ]);
    expect(q.options.map((o) => o.selected)).toEqual([false, true, false, false, false, false, false]);
    expect(q.options.every((o) => o.n === null && !/[✓✔]/.test(o.label))).toBe(true);
    expect(q.yesNo).toBe(false);
  });

  it('shows the heading and the menu as the question, not the code preview or the dotted banner', () => {
    const q = parseScreenQuestion(THEME)!;
    expect(q.lines).toContain('Choose the text style that looks best with your terminal');
    expect(q.lines).toContain('❯ ✔ Dark mode');
    expect(q.lines.join('\n')).not.toMatch(/function greet|Hello, Claude|Syntax theme|\.{4,}|╌/);
    // "Let's get started.", the title, the hint and the seven rows.
    expect(q.lines).toHaveLength(3 + 7);
  });

  it('answers with arrow keys from the row the pointer is on', () => {
    const q = parseScreenQuestion(THEME)!;
    expect(keysForOption(q, 1)).toEqual({ text: '', enter: true });
    expect(keysForOption(q, 0)).toEqual({ text: KEY_UP, enter: true });
    expect(keysForOption(q, 3)).toEqual({ text: KEY_DOWN + KEY_DOWN, enter: true });
  });

  it('reads the mark as a plain check or a bullet', () => {
    for (const mark of ['✓', '●', '◉']) {
      const q = parseScreenQuestion(THEME.replace('❯ ✔ Dark mode', `❯ ${mark} Dark mode`))!;
      expect(q.options).toHaveLength(7);
      expect(q.options[1]).toMatchObject({ label: 'Dark mode', selected: true });
    }
  });

  it('reads the menu when the pointer moved away from the checked row', () => {
    const moved = THEME.replace('❯ ✔ Dark mode', '  ✔ Dark mode').replace('     Light mode\n', ' ❯   Light mode\n');
    expect(moved).not.toBe(THEME);
    const q = parseScreenQuestion(moved)!;
    expect(q.options).toHaveLength(7);
    expect(q.options.map((o) => o.label).slice(0, 3)).toEqual(['Auto (match terminal)', 'Dark mode', 'Light mode']);
    expect(q.options[2].selected).toBe(true);
    expect(keysForOption(q, 1)).toEqual({ text: KEY_UP, enter: true });
  });

  it('reads a marked pointer row in other menus, and checkbox rows', () => {
    const q = parseScreenQuestion('Pick one\n ❯ ● Alpha\n     Beta\n     Gamma\n')!;
    expect(q.options.map((o) => [o.label, o.selected])).toEqual([
      ['Alpha', true],
      ['Beta', false],
      ['Gamma', false],
    ]);
    const boxes = parseScreenQuestion('Pick any\n ❯ [x] Alpha\n   [ ] Beta\n')!;
    expect(boxes.options.map((o) => o.label)).toEqual(['Alpha', 'Beta']);
  });

  it('reads the first-run sign-in menu and the API-key question from their captures', () => {
    const login = parseScreenQuestion(LOGIN_METHOD)!;
    expect(login.options.map((o) => o.label)).toEqual(['Claude account with subscription', 'Anthropic Console account', '3rd-party platform']);
    const key = parseScreenQuestion(API_KEY)!;
    expect(key.options.map((o) => [o.label, o.selected])).toEqual([
      ['Yes', false],
      ['No (recommended)', true],
    ]);
    expect(key.lines.at(-1)).toBe('Enter to confirm · Esc to cancel');
    expect(key.lines.join('\n')).toContain('Do you want to use this API key?');
  });
});

describe('a menu above other content', () => {
  it('finds a menu that is not in the last 12 rows and shows the rows around it', () => {
    const preview = Array.from({ length: 20 }, (_, i) => `  ${i + 1}  preview line ${i}`).join('\n');
    const screen = `Which template?\n❯ Basic\n  Full\n  Empty\n${'─'.repeat(40)}\n${preview}\n`;
    const q = parseScreenQuestion(screen)!;
    expect(q.options.map((o) => o.label)).toEqual(['Basic', 'Full', 'Empty']);
    expect(q.lines).toEqual(['Which template?', '❯ Basic', 'Full', 'Empty']);
  });

  it('stops a menu at a hint line under it', () => {
    const q = parseScreenQuestion('Pick a mode\n❯ Fast\n  Careful\n  Esc to cancel\n  ctrl+t to toggle\n')!;
    expect(q.options.map((o) => o.label)).toEqual(['Fast', 'Careful']);
  });

  it('leaves a numbered list far above, with no cursor, out of the question', () => {
    const rows = ['Steps:', '1. Install', '2. Build', ...Array.from({ length: 30 }, (_, i) => `output ${i}`)];
    expect(parseScreenQuestion(rows.join('\n'))!.options).toEqual([]);
  });
});

describe('rules that are not text', () => {
  it('drops rows of dots, dashes and box characters from the question', () => {
    const screen = ['.....................', 'Overwrite the file? (y/n)', '-------------', '╌╌╌╌╌╌╌╌╌╌╌╌', '=============='].join('\n');
    const q = parseScreenQuestion(screen)!;
    expect(q.lines).toEqual(['Overwrite the file? (y/n)']);
    expect(q.yesNo).toBe(true);
  });

  it('keeps a row that only ends in dots', () => {
    expect(parseScreenQuestion('Loading...\nName the project:')!.lines).toEqual(['Loading...', 'Name the project:']);
  });
});

describe('strict: an idle pane is only asked about a real menu', () => {
  const idleBoxes: Array<[string, string]> = [
    ['claude, fresh', 'tui/claude/fixtures/first_run_idle_box_2_1_312.txt'],
    ['claude, ready', 'tui/claude/fixtures/ready_fresh_2_1_299_live.txt'],
    ['claude, after a turn', 'tui/claude/fixtures/ready_after_turn_2_1_299_live.txt'],
    ['codex, ready', 'tui/codex/fixtures/standby_ready_live.txt'],
    ['codex, text typed', 'tui/codex/fixtures/composer_unsent_live.txt'],
    ['cursor', 'tui/cursor/fixtures/input_box_unframed_live.txt'],
    ['opencode', 'tui/opencode/fixtures/screen_home.txt'],
    ['kilo', 'agent_runs/fixtures/kilo_home_typed_121.txt'],
  ];

  it.each(idleBoxes)('does not take the idle input box of %s for a menu', (_name, path) => {
    const screen = fixture(path);
    expect(parseScreenQuestion(screen, { strict: true })).toBeNull();
    // Not even loosely: the box has no options.
    expect(parseScreenQuestion(screen)?.options ?? []).toEqual([]);
  });

  it('does not take a box with a "? for shortcuts" hint for a menu', () => {
    const framed = ['Done. Anything else?', '', '╭────────────────────────────╮', '│ > Try "fix lint errors"    │', '╰────────────────────────────╯', '  ? for shortcuts'].join('\n');
    expect(parseScreenQuestion(framed, { strict: true })).toBeNull();
    const bare = ['> Try "fix lint errors"', '  ? for shortcuts', '  gpt · ~/work/demo'].join('\n');
    expect(parseScreenQuestion(bare, { strict: true })).toBeNull();
    expect(parseScreenQuestion(bare)!.options).toEqual([]);
  });

  it('reads a menu with a cursor near the end of the screen', () => {
    const q = parseScreenQuestion(THEME, { strict: true })!;
    expect(q.options).toHaveLength(7);
    expect(q.options[1].selected).toBe(true);
    expect(parseScreenQuestion(LOGIN_METHOD, { strict: true })!.options).toHaveLength(3);
    expect(parseScreenQuestion(API_KEY, { strict: true })!.options).toHaveLength(2);
  });

  it('gives nothing for a list with no cursor, a y/n line or plain text', () => {
    expect(parseScreenQuestion('Which one?\n  1) npm\n  2) pnpm\n', { strict: true })).toBeNull();
    expect(parseScreenQuestion('Overwrite config.json? (y/n)\n', { strict: true })).toBeNull();
    expect(parseScreenQuestion('What should the project be called?\n>\n', { strict: true })).toBeNull();
    expect(parseScreenQuestion('All done. 12 tests passed.\n', { strict: true })).toBeNull();
  });

  it('gives nothing for a menu with a cursor that scrolled far up the screen', () => {
    const rows = ['Pick', '❯ A', '  B', ...Array.from({ length: 20 }, (_, i) => `later output ${i}`)];
    expect(parseScreenQuestion(rows.join('\n'), { strict: true })).toBeNull();
  });
});
