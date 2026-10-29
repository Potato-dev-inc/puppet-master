// Reads what a waiting terminal asks, from its screen text, for the Chat card "The agent is asking".
//
// Generic on purpose: it knows numbers, a pointer row and "y/n", never an agent or a program. A profile that
// recognises its own dialog already shows a proper card; this is what the person gets for every other question,
// so nothing waits in the terminal where Chat cannot see it.
//
// Pure: screen text in, a plain description out. The keys that answer are built here too, so a test can pin them.

/** How many of the last meaningful rows are shown as the question. */
export const MAX_QUESTION_LINES = 12;
/** Longest option label kept (the rest is cut with an ellipsis). */
const MAX_LABEL = 80;

export interface ScreenOption {
  /** The number the row shows (`1`, `2`), or null for a row with no number. */
  n: number | null;
  /** What the row says, without its number, pointer or small print after " · ". */
  label: string;
  /** The small print after " · " (kept for a tooltip), or ''. */
  detail: string;
  /** The pointer (❯ or >) is on this row. */
  selected: boolean;
}

export interface ScreenQuestion {
  /** The last meaningful rows of the screen, cleaned: no colours, no box borders, no blank rows. */
  lines: string[];
  /** The rows of a menu (numbered or with a pointer), in the order shown. Empty when there is no menu. */
  options: ScreenOption[];
  /** A yes / no question with no menu: "(y/n)", "[Y/n]", "Yes/No". */
  yesNo: boolean;
}

// eslint-disable-next-line no-control-regex
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]|\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)|\u001b[@-Z\\-_]/g;
/** Box drawing and block elements: borders, rules, bars. */
const BOX = /[─-▟]/g;
const POINTER = /^[❯›▶▸>]\s+/;
const NUMBERED = /^(\d{1,2})[.)]\s+(\S.*)$/;
const YES_NO = /\((?:y\/n|yes\/no)\)|\[(?:y\/n|yes\/no)\]|\b(?:y\/n|yes\/no)\b/i;

/** The screen as rows: colours and borders gone, trailing space cut, indent kept (a menu's rows line up by it). */
function cleanRows(screen: string): string[] {
  const rows: string[] = [];
  for (const raw of screen.replace(ANSI, '').replace(/\r/g, '').split('\n')) {
    const row = raw
      .replace(BOX, ' ')
      .replace(/[│|]\s*$/, '')
      .trimEnd();
    if (row.trim() !== '') rows.push(row);
  }
  return rows;
}

function shorten(text: string): string {
  return text.length > MAX_LABEL ? `${text.slice(0, MAX_LABEL - 1).trimEnd()}…` : text;
}

function makeOption(n: number | null, text: string, selected: boolean): ScreenOption {
  const [head, ...rest] = text.split(/\s+·\s+/);
  return {
    n,
    label: shorten(head.trim()),
    detail: rest.join(' · ').trim(),
    selected,
  };
}

/** The last run of rows numbered 1, 2, 3 ... (descriptions may sit between them). */
function numberedOptions(rows: string[]): ScreenOption[] {
  let current: ScreenOption[] = [];
  let last: ScreenOption[] = [];
  for (const row of rows) {
    let text = row.trim();
    const pointer = POINTER.test(text);
    if (pointer) text = text.replace(POINTER, '');
    const m = NUMBERED.exec(text);
    if (!m) continue;
    const n = Number(m[1]);
    if (n === 1) current = [];
    else if (current.length === 0 || current[current.length - 1].n !== n - 1) continue;
    current.push(makeOption(n, m[2], pointer));
    if (current.length >= 2) last = current.slice();
  }
  return last;
}

/** A menu whose rows have no numbers: the pointer row and the rows around it at the same indent. */
function pointerOptions(rows: string[]): ScreenOption[] {
  const at = rows.map((r) => POINTER.test(r.trim())).lastIndexOf(true);
  if (at < 0) return [];
  const indentOf = (row: string) => row.length - row.trimStart().length;
  const textOf = (row: string) => row.trim().replace(POINTER, '');
  // Text starts after the pointer and one space; other rows of the menu start at the same column.
  const column = indentOf(rows[at]) + (rows[at].trim().length - textOf(rows[at]).length);
  let from = at;
  while (from > 0 && indentOf(rows[from - 1]) === column && !POINTER.test(rows[from - 1].trim())) from--;
  let to = at;
  while (to < rows.length - 1 && indentOf(rows[to + 1]) === column && !POINTER.test(rows[to + 1].trim())) to++;
  const group = rows.slice(from, to + 1);
  if (group.length < 2 || group.length > 12) return [];
  return group.map((row, i) => makeOption(null, textOf(row), from + i === at));
}

/**
 * What the screen asks, or null for a screen with nothing on it. A screen with text but no menu and no yes / no is a
 * free-text question: only `lines` are set, and the person types the answer.
 */
export function parseScreenQuestion(screen: string): ScreenQuestion | null {
  const rows = cleanRows(screen);
  if (rows.length === 0) return null;
  const tail = rows.slice(-MAX_QUESTION_LINES);
  let options = numberedOptions(tail);
  if (options.length === 0) options = pointerOptions(tail);
  const yesNo = options.length === 0 && tail.slice(-4).some((row) => YES_NO.test(row));
  const lines = tail.map((row) => row.trim());
  // Keep the shape of a menu readable: the rows are shown as the screen has them, one indent level at most.
  return { lines, options, yesNo };
}

export const KEY_UP = '\u001b[A';
export const KEY_DOWN = '\u001b[B';
export const KEY_ESC = '\u001b';
export const KEY_TAB = '\t';

/** What to type for an answer: `text` first (no Enter), then `enter` if set. */
export interface KeyPlan {
  text: string;
  enter: boolean;
}

/**
 * The keys that pick `options[index]`. A menu with a pointer is moved with arrow keys and then Enter (a number key
 * can pick and submit on its own, and the Enter after it would answer the next question). A menu with numbers and
 * no pointer takes the number and Enter, like a prompt that reads a line.
 */
export function keysForOption(question: ScreenQuestion, index: number): KeyPlan | null {
  const target = question.options[index];
  if (!target) return null;
  const from = question.options.findIndex((o) => o.selected);
  if (from >= 0) {
    const steps = index - from;
    return {
      text: (steps >= 0 ? KEY_DOWN : KEY_UP).repeat(Math.abs(steps)),
      enter: true,
    };
  }
  if (target.n !== null) return { text: String(target.n), enter: true };
  return null;
}

/** Yes or No for a "(y/n)" question: the letter and Enter. */
export function keysForYesNo(yes: boolean): KeyPlan {
  return { text: yes ? 'y' : 'n', enter: true };
}
