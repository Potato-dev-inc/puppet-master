// The agent's text as a small block tree, for the chat view. Pure: no React and no HTML. The renderer turns the
// tree into elements, so text is only ever text (a `<script>` in the message is shown as typed). Anything the
// parser does not recognise stays plain text, and it never throws: every scan is bounded so a long text stays
// close to linear.

export type Align = 'left' | 'center' | 'right' | null;

export type Inline =
  | { kind: 'text'; text: string }
  | { kind: 'break' }
  | { kind: 'code'; text: string }
  | { kind: 'strong'; children: Inline[] }
  | { kind: 'em'; children: Inline[] }
  | { kind: 'link'; url: string; children: Inline[] };

export interface ListItem {
  children: Inline[];
  /** One nested level, under this item. */
  sub: ListBlock | null;
}

export interface ListBlock {
  kind: 'list';
  ordered: boolean;
  start: number;
  items: ListItem[];
}

export type Block =
  | { kind: 'paragraph'; children: Inline[] }
  | { kind: 'heading'; level: 1 | 2 | 3; children: Inline[] }
  | { kind: 'code'; lang: string; text: string }
  | ListBlock
  | { kind: 'quote'; children: Block[] }
  | { kind: 'rule' }
  | { kind: 'table'; header: Inline[][]; align: Align[]; rows: Inline[][][] };

const MAX_QUOTE_DEPTH = 3;
const MAX_INLINE_DEPTH = 3;
/** How far an emphasis closer is looked for. A marker with no closer within this stays plain text. */
const EMPHASIS_SPAN = 1000;
const LINK_TEXT_SPAN = 300;
const LINK_URL_SPAN = 1000;

const FENCE = /^ {0,3}```([^`]*)$/;
const FENCE_CLOSE = /^ {0,3}```\s*$/;
const HEADING = /^ {0,3}(#{1,3})\s+(.*?)\s*$/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}>/;
const QUOTE_PREFIX = /^ {0,3}> ?/;
const LIST = /^( *)(?:[-*+]|(\d{1,9})[.)])\s+(.*)$/;
const TABLE_SEP = /^ *\|? *:?-+:? *(?:\| *:?-+:? *)*\|? *$/;
const PUNCT = /[!-/:-@[-`{-~]/;

interface Hit {
  node: Inline;
  end: number;
}

interface ListLine {
  indent: number;
  ordered: boolean;
  num: number;
  text: string;
}

interface RawItem {
  lines: string[];
  kids: RawItem[];
  ordered: boolean;
  num: number;
}

/** Parses a whole chat text into blocks. */
export function parseChatMarkdown(text: string): Block[] {
  const lines = text.replace(/\r\n?/g, '\n').replace(/\t/g, '    ').split('\n');
  return parseBlocks(lines, 0);
}

function isBlank(line: string): boolean {
  return line.trim() === '';
}

function isTableStart(lines: string[], i: number): boolean {
  const next = lines[i + 1];
  return next !== undefined && lines[i].includes('|') && next.includes('|') && TABLE_SEP.test(next);
}

/** Whether a line starts a block of its own (so a paragraph ends before it). */
function startsBlock(lines: string[], i: number, depth: number): boolean {
  const line = lines[i];
  return (
    FENCE.test(line) ||
    HEADING.test(line) ||
    RULE.test(line) ||
    (depth < MAX_QUOTE_DEPTH && QUOTE.test(line)) ||
    LIST.test(line) ||
    isTableStart(lines, i)
  );
}

function parseBlocks(lines: string[], depth: number): Block[] {
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (isBlank(line)) {
      i++;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const close = findFenceClose(lines, i + 1);
      if (close >= 0) {
        const lang = (fence[1] ?? '').trim().split(/\s+/)[0] ?? '';
        out.push({ kind: 'code', lang, text: lines.slice(i + 1, close).join('\n') });
        i = close + 1;
        continue;
      }
      // An unclosed fence stays plain text (the paragraph below takes this line).
    }
    const heading = HEADING.exec(line);
    if (heading) {
      const level = (heading[1] ?? '#').length as 1 | 2 | 3;
      out.push({ kind: 'heading', level, children: parseInline(heading[2] ?? '') });
      i++;
      continue;
    }
    if (RULE.test(line)) {
      out.push({ kind: 'rule' });
      i++;
      continue;
    }
    if (depth < MAX_QUOTE_DEPTH && QUOTE.test(line)) {
      const inner: string[] = [];
      let j = i;
      while (j < lines.length && QUOTE.test(lines[j])) {
        inner.push(lines[j].replace(QUOTE_PREFIX, ''));
        j++;
      }
      out.push({ kind: 'quote', children: parseBlocks(inner, depth + 1) });
      i = j;
      continue;
    }
    if (isTableStart(lines, i)) {
      const table = parseTable(lines, i);
      out.push(table.block);
      i = table.next;
      continue;
    }
    if (LIST.test(line)) {
      const list = parseList(lines, i);
      out.push(list.block);
      i = list.next;
      continue;
    }
    // A paragraph: its first line always, then the lines up to the next block or a blank line.
    const para = [line.trim()];
    let j = i + 1;
    while (j < lines.length && !isBlank(lines[j]) && !startsBlock(lines, j, depth)) {
      para.push(lines[j].trim());
      j++;
    }
    out.push({ kind: 'paragraph', children: parseInline(para.join('\n')) });
    i = j;
  }
  return out;
}

function findFenceClose(lines: string[], from: number): number {
  for (let j = from; j < lines.length; j++) {
    if (FENCE_CLOSE.test(lines[j])) return j;
  }
  return -1;
}

function splitRow(line: string): string[] {
  let s = line.trim();
  if (s.startsWith('|')) s = s.slice(1);
  if (s.endsWith('|') && !s.endsWith('\\|')) s = s.slice(0, -1);
  const cells: string[] = [];
  let cur = '';
  for (let k = 0; k < s.length; k++) {
    const ch = s[k];
    if (ch === '\\' && s[k + 1] === '|') {
      cur += '|';
      k++;
    } else if (ch === '|') {
      cells.push(cur.trim());
      cur = '';
    } else {
      cur += ch;
    }
  }
  cells.push(cur.trim());
  return cells;
}

function alignOf(cell: string | undefined): Align {
  if (cell === undefined) return null;
  const left = cell.startsWith(':');
  const right = cell.endsWith(':');
  if (left && right) return 'center';
  if (right) return 'right';
  if (left) return 'left';
  return null;
}

function parseTable(lines: string[], i: number): { block: Block; next: number } {
  const header = splitRow(lines[i]);
  const sep = splitRow(lines[i + 1] ?? '');
  const rows: Inline[][][] = [];
  let j = i + 2;
  while (j < lines.length && !isBlank(lines[j]) && lines[j].includes('|')) {
    const cells = splitRow(lines[j]);
    rows.push(header.map((_, k) => parseInline(cells[k] ?? '')));
    j++;
  }
  const block: Block = {
    kind: 'table',
    header: header.map((c) => parseInline(c)),
    align: header.map((_, k) => alignOf(sep[k])),
    rows,
  };
  return { block, next: j };
}

function listLine(line: string): ListLine | null {
  const m = LIST.exec(line);
  if (!m) return null;
  const ordered = m[2] !== undefined;
  return {
    indent: (m[1] ?? '').length,
    ordered,
    num: ordered ? parseInt(m[2] ?? '1', 10) : 1,
    text: m[3] ?? '',
  };
}

function nextNonBlank(lines: string[], from: number): number {
  let k = from;
  while (k < lines.length && isBlank(lines[k])) k++;
  return k;
}

/** A list from `start`: top-level items (indent under 2), one nested level under each (indent 2 or more). */
function parseList(lines: string[], start: number): { block: ListBlock; next: number } {
  const head = listLine(lines[start]) ?? { indent: 0, ordered: false, num: 1, text: '' };
  const top: RawItem[] = [];
  let i = start;
  while (i < lines.length) {
    const line = lines[i];
    if (isBlank(line)) {
      // A blank line keeps the list going only when the next item is a top-level one of the same kind.
      const k = nextNonBlank(lines, i);
      const next = k < lines.length ? listLine(lines[k]) : null;
      if (next && next.indent < 2 && next.ordered === head.ordered) {
        i = k;
        continue;
      }
      break;
    }
    const item = listLine(line);
    if (item && (item.indent < 2 || top.length === 0)) {
      if (item.ordered !== head.ordered) break;
      top.push({ lines: [item.text], kids: [], ordered: item.ordered, num: item.num });
      i++;
      continue;
    }
    if (item) {
      const parent = top[top.length - 1];
      if (!parent) break;
      parent.kids.push({ lines: [item.text], kids: [], ordered: item.ordered, num: item.num });
      i++;
      continue;
    }
    // An indented line right after an item continues that item (its nested item when it has one).
    if (/^ {2,}\S/.test(line) && top.length > 0 && !startsBlock(lines, i, MAX_QUOTE_DEPTH)) {
      const last = top[top.length - 1];
      const tail = last && last.kids.length > 0 ? last.kids[last.kids.length - 1] : last;
      tail?.lines.push(line.trim());
      i++;
      continue;
    }
    break;
  }
  return {
    block: { kind: 'list', ordered: head.ordered, start: head.num, items: top.map(toItem) },
    next: i,
  };
}

function toItem(raw: RawItem): ListItem {
  const first = raw.kids[0];
  return {
    children: parseInline(raw.lines.join('\n')),
    sub: first
      ? { kind: 'list', ordered: first.ordered, start: first.num, items: raw.kids.map(toItem) }
      : null,
  };
}

function runLength(src: string, at: number, ch: string): number {
  let r = 0;
  while (src[at + r] === ch) r++;
  return r;
}

/** The first run of exactly `want` copies of `ch` from `from` (before `end`) that `ok` accepts. */
function findRun(
  src: string,
  from: number,
  ch: string,
  want: number,
  end: number,
  ok: (p: number, r: number) => boolean,
): number {
  let p = from;
  while (p < end) {
    if (src[p] === ch) {
      const r = runLength(src, p, ch);
      if (r === want && ok(p, r)) return p;
      p += r;
    } else {
      p++;
    }
  }
  return -1;
}

/** Index of `ch` in the window of `span` characters from `from`, or -1. */
function indexIn(src: string, ch: string, from: number, span: number): number {
  const p = src.slice(from, from + span).indexOf(ch);
  return p < 0 ? -1 : from + p;
}

function isSpace(ch: string | undefined): boolean {
  return ch === undefined || /\s/.test(ch);
}

function isWord(ch: string | undefined): boolean {
  return ch !== undefined && /[\p{L}\p{N}]/u.test(ch);
}

function codeText(s: string): string {
  const t = s.replace(/\n/g, ' ');
  return t.length > 2 && t.startsWith(' ') && t.endsWith(' ') && t.trim() !== '' ? t.slice(1, -1) : t;
}

/** Inline text: code spans, **strong**, *em*, _em_, [text](url) and backslash escapes. Unclosed markers stay text. */
export function parseInline(src: string, depth = 0): Inline[] {
  const out: Inline[] = [];
  let buf = '';
  const flush = () => {
    if (buf) {
      out.push({ kind: 'text', text: buf });
      buf = '';
    }
  };
  let i = 0;
  while (i < src.length) {
    const c = src[i] ?? '';
    if (c === '\\' && i + 1 < src.length && PUNCT.test(src[i + 1] ?? '')) {
      buf += src[i + 1];
      i += 2;
      continue;
    }
    if (c === '\n') {
      flush();
      out.push({ kind: 'break' });
      i++;
      continue;
    }
    if (c === '`') {
      const run = runLength(src, i, '`');
      const close = findRun(src, i + run, '`', run, src.length, () => true);
      if (close >= 0) {
        flush();
        out.push({ kind: 'code', text: codeText(src.slice(i + run, close)) });
        i = close + run;
      } else {
        buf += '`'.repeat(run);
        i += run;
      }
      continue;
    }
    if (depth < MAX_INLINE_DEPTH) {
      const hit = c === '[' ? parseLink(src, i, depth) : c === '*' || c === '_' ? parseEmphasis(src, i, depth) : null;
      if (hit) {
        flush();
        out.push(hit.node);
        i = hit.end;
        continue;
      }
    }
    buf += c;
    i++;
  }
  flush();
  return out;
}

function parseEmphasis(src: string, i: number, depth: number): Hit | null {
  const c = src[i] ?? '';
  const run = runLength(src, i, c);
  if (run > 3) return null;
  if (isSpace(src[i + run]) || (c === '_' && isWord(src[i - 1]))) return null;
  const underscore = c === '_';
  const close = findRun(
    src,
    i + run,
    c,
    run,
    Math.min(src.length, i + EMPHASIS_SPAN),
    (p, r) => !isSpace(src[p - 1]) && !(underscore && isWord(src[p + r])),
  );
  if (close < 0) return null;
  const children = parseInline(src.slice(i + run, close), depth + 1);
  if (run === 1) return { node: { kind: 'em', children }, end: close + 1 };
  if (run === 2) return { node: { kind: 'strong', children }, end: close + 2 };
  return { node: { kind: 'strong', children: [{ kind: 'em', children }] }, end: close + 3 };
}

function parseLink(src: string, i: number, depth: number): Hit | null {
  const close = indexIn(src, ']', i + 1, LINK_TEXT_SPAN);
  if (close < 0 || close === i + 1 || src[close + 1] !== '(') return null;
  const paren = indexIn(src, ')', close + 2, LINK_URL_SPAN);
  if (paren < 0) return null;
  const url = (src.slice(close + 2, paren).trim().split(/\s+/)[0] ?? '').trim();
  if (!url) return null;
  return {
    node: { kind: 'link', url, children: parseInline(src.slice(i + 1, close), depth + 1) },
    end: paren + 1,
  };
}
