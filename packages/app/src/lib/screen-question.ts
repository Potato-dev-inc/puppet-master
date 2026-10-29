// Reads what a waiting terminal asks, from its screen text, for the Chat card "The agent is asking".
//
// Generic on purpose: it knows numbers, a pointer, a mark (a check, a bullet), a rule and "y/n", never an agent or a
// program. A profile that recognises its own dialog already shows a proper card; this is what the person gets for
// every other question, so nothing waits in the terminal where Chat cannot see it.
//
// Pure: screen text in, a plain description out. The keys that answer are built here too, so a test can pin them.

/** How many of the last meaningful rows are shown when the screen holds no menu. */
export const MAX_QUESTION_LINES = 12;
/** Longest option label kept (the rest is cut with an ellipsis). */
const MAX_LABEL = 80;
/** How many of the last rows are searched for a menu (a preview or a hint can sit under it). */
export const MENU_WINDOW = 40;
/** A menu this close to the end of the screen (in rows) counts for a pane that is not waiting. */
const NEAR_END = 8;
/** Rows of the question shown above a menu. */
const ROWS_ABOVE = 4;
/** Rows of small print shown right under a menu (its footer). */
const ROWS_BELOW = 2;

export interface ScreenOption {
  /** The number the row shows (`1`, `2`), or null for a row with no number. */
  n: number | null;
  /** What the row says, without its number, pointer, mark or small print after " · ". */
  label: string;
  /** The small print after " · " (kept for a tooltip), or ''. */
  detail: string;
  /** The pointer (❯ or >) is on this row. */
  selected: boolean;
}

export interface ScreenQuestion {
  /**
   * The rows that carry the question, cleaned: no colours, no box borders, no blank rows. With a menu: the menu, the
   * rows right above it and its footer (not a preview under it). Without one: the last rows of the screen.
   */
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
/** A mark after the pointer or in front of a row: a check, a bullet, a box (`✔`, `●`, `[x]`). */
const MARK = '(?:[✓✔●◉○◯]|\\[[ xX✓✔]\\])';
const POINTER = new RegExp(`^[❯›▶▸>]\\s+(?:${MARK}\\s+)?`);
const MARKED = new RegExp(`^${MARK}\\s+`);
const NUMBERED = /^(\d{1,2})[.)]\s+(\S.*)$/;
const YES_NO = /\((?:y\/n|yes\/no)\)|\[(?:y\/n|yes\/no)\]|\b(?:y\/n|yes\/no)\b/i;
/** A row that is a rule made of dots, dashes, equal signs or the like. */
const DOTTED_RULE = /^[\s.\-_=~·•*]+$/;
/** A hint or status line under an input box, which is never a row of a menu. */
const HINT =
  /\b(?:ctrl|shift|alt|cmd)\+|\besc\b|\bto (?:cycle|interrupt|navigate|select|confirm|cancel|exit)\b|^\?\s|\s·\s.*[\\/~]/i;

interface Layout {
  /** `gaps[i]`: a rule (a border, a row of dots) sits right before `rows[i]`; `gaps[rows.length]`: after the last. */
  gaps: boolean[];
  /** `blanks[i]`: how many blank rows sit right before `rows[i]`. */
  blanks: number[];
}

interface Rows extends Layout {
  /** The screen as rows: colours and borders gone, trailing space cut, indent kept (a menu's rows line up by it). */
  rows: string[];
}

function cleanRows(screen: string): Rows {
  const rows: string[] = [];
  const gaps: boolean[] = [false];
  const blanks: number[] = [0];
  let blank = 0;
  for (const raw of screen.replace(ANSI, '').replace(/\r/g, '').split('\n')) {
    const row = raw
      .replace(BOX, ' ')
      .replace(/[│|]\s*$/, '')
      .trimEnd();
    const bare = row.trim();
    if (bare === '') {
      if (raw.trim() !== '') gaps[rows.length] = true;
      else blank++;
      continue;
    }
    if (DOTTED_RULE.test(bare) && bare.replace(/\s/g, '').length >= 5) {
      gaps[rows.length] = true;
      continue;
    }
    blanks[rows.length] = blank;
    blank = 0;
    rows.push(row);
    gaps.push(false);
    blanks.push(0);
  }
  return { rows, gaps, blanks };
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

/** Where a menu was found: its options and the rows (indexes into the screen's rows) it spans. */
interface Found {
  options: ScreenOption[];
  from: number;
  to: number;
}

/** The last run of rows numbered 1, 2, 3 ... (descriptions may sit between them). */
function numberedOptions(rows: string[], first: number): Found | null {
  let current: ScreenOption[] = [];
  let currentFrom = 0;
  let last: Found | null = null;
  for (let i = first; i < rows.length; i++) {
    let text = rows[i].trim();
    const pointer = POINTER.test(text);
    if (pointer) text = text.replace(POINTER, '');
    const m = NUMBERED.exec(text);
    if (!m) continue;
    const n = Number(m[1]);
    if (n === 1) {
      current = [];
      currentFrom = i;
    } else if (current.length === 0 || current[current.length - 1].n !== n - 1) continue;
    current.push(makeOption(n, m[2], pointer));
    if (current.length >= 2) last = { options: current.slice(), from: currentFrom, to: i };
  }
  return last;
}

const indentOf = (row: string) => row.length - row.trimStart().length;

/** A menu row without its pointer or mark: the column where its words start, and what they say. */
function menuRow(row: string): {
  column: number;
  text: string;
  pointer: boolean;
} {
  const trimmed = row.trim();
  const lead = indentOf(row);
  const pointer = POINTER.exec(trimmed);
  if (pointer)
    return {
      column: lead + pointer[0].length,
      text: trimmed.slice(pointer[0].length),
      pointer: true,
    };
  const marked = MARKED.exec(trimmed);
  if (marked)
    return {
      column: lead + marked[0].length,
      text: trimmed.slice(marked[0].length),
      pointer: false,
    };
  return { column: lead, text: trimmed, pointer: false };
}

/**
 * A menu whose rows have no numbers: the pointer row and the rows around it at the same column. The words of the
 * pointer row start after the pointer and after a mark (`❯ ✔ Dark mode`); the other rows start at that column too. A
 * rule or a hint line ends the menu on either side.
 */
function pointerOptions(rows: string[], layout: Layout, first: number, strict: boolean): Found | null {
  let at = -1;
  for (let i = rows.length - 1; i >= first; i--) {
    if (POINTER.test(rows[i].trim())) {
      at = i;
      break;
    }
  }
  if (at < 0) return null;
  const { gaps, blanks } = layout;
  const column = menuRow(rows[at]).column;
  // Rows of a menu are close: a rule never sits between them, and one blank row at most (none, for a pane that is only idle).
  // A row with a mark in front joins only when it touches the group.
  const joins = (i: number, link: number) => {
    const row = menuRow(rows[i]);
    if (row.column !== column || row.pointer || HINT.test(row.text)) return false;
    if (gaps[link] || blanks[link] > (strict ? 0 : 1)) return false;
    return !(MARKED.test(rows[i].trim()) && blanks[link] > 0);
  };
  let from = at;
  while (from > first && joins(from - 1, from)) from--;
  let to = at;
  while (to < rows.length - 1 && joins(to + 1, to + 1)) to++;
  const count = to - from + 1;
  if (count < 2 || count > 12) return null;
  const options: ScreenOption[] = [];
  for (let i = from; i <= to; i++) options.push(makeOption(null, menuRow(rows[i]).text, i === at));
  return { options, from, to };
}

export interface ParseOptions {
  /**
   * For a pane that is not waiting (idle): only a menu with a cursor, close to the end of the screen, counts. The
   * input box of an agent and the list in a finished reply are not menus. A waiting pane asked for a hand, so it
   * takes any menu, a y/n question or a free-text line.
   */
  strict?: boolean;
}

/**
 * What the screen asks, or null for a screen with nothing on it (or, when `strict`, nothing that is a menu). A
 * screen with text but no menu and no yes / no is a free-text question: only `lines` are set, and the person types
 * the answer.
 */
export function parseScreenQuestion(screen: string, opts: ParseOptions = {}): ScreenQuestion | null {
  const { rows, gaps, blanks } = cleanRows(screen);
  if (rows.length === 0) return null;
  const strict = opts.strict === true;
  const windowStart = Math.max(0, rows.length - MENU_WINDOW);
  let found = numberedOptions(rows, windowStart);
  // A numbered list far up with no cursor is an old reply, not the question.
  if (found && !found.options.some((o) => o.selected) && found.to < rows.length - MAX_QUESTION_LINES) found = null;
  if (!found) found = pointerOptions(rows, { gaps, blanks }, windowStart, strict);
  if (found && strict && (!found.options.some((o) => o.selected) || found.to < rows.length - NEAR_END)) found = null;
  if (strict && !found) return null;

  if (!found) {
    const tail = rows.slice(-MAX_QUESTION_LINES);
    const yesNo = tail.slice(-4).some((row) => YES_NO.test(row));
    return { lines: tail.map((row) => row.trim()), options: [], yesNo };
  }
  // The question is the menu with the rows above it (down to a rule) and its footer: not a preview under a rule.
  let start = found.from;
  for (let k = 0; k < ROWS_ABOVE && start > 0 && !gaps[start]; k++) start--;
  let end = found.to;
  for (let k = 0; k < ROWS_BELOW && end < rows.length - 1 && !gaps[end + 1]; k++) end++;
  return {
    lines: rows.slice(start, end + 1).map((row) => row.trim()),
    options: found.options,
    yesNo: false,
  };
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
