import { describe, expect, it } from 'vitest';
import { KEY_DOWN, KEY_UP, keysForOption, keysForYesNo, parseScreenQuestion } from './screen-question';

const LOGIN = `
 Login

 Claude Code can be used with your Claude subscription or billed based on API usage through your Console account.

 Select login method:

 ❯ 1. Claude account with subscription · Pro, Max, Team, or Enterprise

   2. Anthropic Console account · API usage billing

   3. 3rd-party platform · Amazon Bedrock, Microsoft Foundry, Google Vertex AI

 Esc to cancel
`;

const NUMBERED_NO_POINTER = `
Which package manager should I use?
  1) npm
  2) pnpm
  3) yarn
Enter a number:
`;

const BOXED_POINTER = `
╭──────────────────╮
│ Do you trust this folder?   │
│                             │
│ ❯ Yes, I trust it           │
│   No, exit                  │
╰──────────────────╯
`;

const YES_NO = `
Wrote 3 files.
Overwrite config.json? (y/n)
`;

const FREE_TEXT = `
Setting up your project.
What should the project be called?
>
`;

const NO_QUESTION = `
\u001b[32mAll done.\u001b[0m 12 tests passed.
Built in 4.1s.
`;

describe('parseScreenQuestion', () => {
  it('reads the login menu as three options with the pointer on the first', () => {
    const q = parseScreenQuestion(LOGIN)!;
    expect(q.options.map((o) => [o.n, o.label, o.selected])).toEqual([
      [1, 'Claude account with subscription', true],
      [2, 'Anthropic Console account', false],
      [3, '3rd-party platform', false],
    ]);
    expect(q.options[0].detail).toBe('Pro, Max, Team, or Enterprise');
    expect(q.yesNo).toBe(false);
    expect(q.lines).toContain('Select login method:');
  });

  it('moves the pointer with arrow keys and then Enter', () => {
    const q = parseScreenQuestion(LOGIN)!;
    expect(keysForOption(q, 0)).toEqual({ text: '', enter: true });
    expect(keysForOption(q, 2)).toEqual({ text: KEY_DOWN + KEY_DOWN, enter: true });
    const moved = { ...q, options: q.options.map((o, i) => ({ ...o, selected: i === 2 })) };
    expect(keysForOption(moved, 0)).toEqual({ text: KEY_UP + KEY_UP, enter: true });
    expect(keysForOption(q, 9)).toBeNull();
  });

  it('reads a numbered list without a pointer and answers with the number', () => {
    const q = parseScreenQuestion(NUMBERED_NO_POINTER)!;
    expect(q.options.map((o) => o.label)).toEqual(['npm', 'pnpm', 'yarn']);
    expect(q.options.every((o) => !o.selected)).toBe(true);
    expect(keysForOption(q, 1)).toEqual({ text: '2', enter: true });
  });

  it('reads a boxed menu with a pointer and no numbers', () => {
    const q = parseScreenQuestion(BOXED_POINTER)!;
    expect(q.options.map((o) => [o.label, o.selected])).toEqual([
      ['Yes, I trust it', true],
      ['No, exit', false],
    ]);
    expect(q.lines.join('\n')).not.toMatch(/[─-╿]/);
    expect(keysForOption(q, 1)).toEqual({ text: KEY_DOWN, enter: true });
  });

  it('reads a (y/n) question as yes / no', () => {
    const q = parseScreenQuestion(YES_NO)!;
    expect(q.yesNo).toBe(true);
    expect(q.options).toEqual([]);
    expect(keysForYesNo(true)).toEqual({ text: 'y', enter: true });
    expect(keysForYesNo(false)).toEqual({ text: 'n', enter: true });
    expect(parseScreenQuestion('Continue? [Y/n]')!.yesNo).toBe(true);
    expect(parseScreenQuestion('Continue? Yes/No')!.yesNo).toBe(true);
  });

  it('keeps a free-text question as plain lines with nothing to pick', () => {
    const q = parseScreenQuestion(FREE_TEXT)!;
    expect(q.options).toEqual([]);
    expect(q.yesNo).toBe(false);
    expect(q.lines).toContain('What should the project be called?');
  });

  it('shows a screen with no question as plain lines, colours removed', () => {
    const q = parseScreenQuestion(NO_QUESTION)!;
    expect(q.options).toEqual([]);
    expect(q.yesNo).toBe(false);
    expect(q.lines[0]).toBe('All done. 12 tests passed.');
  });

  it('gives nothing for a blank screen and keeps at most 12 rows', () => {
    expect(parseScreenQuestion('')).toBeNull();
    expect(parseScreenQuestion('  \n ──── \n\n')).toBeNull();
    const long = Array.from({ length: 40 }, (_, i) => `row ${i}`).join('\n');
    const q = parseScreenQuestion(long)!;
    expect(q.lines).toHaveLength(12);
    expect(q.lines[11]).toBe('row 39');
  });

  it('does not take the message box for a menu', () => {
    const q = parseScreenQuestion('Done.\n> fix the tests\n')!;
    expect(q.options).toEqual([]);
  });
});
