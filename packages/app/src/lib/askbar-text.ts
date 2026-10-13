// Splits an Ask bar answer into prose and fenced code blocks. Pure, no DOM.

export type TextBlock = { kind: 'text'; text: string } | { kind: 'code'; text: string; lang: string };

const OPEN_FENCE = /^\s*```\s*([^\s`]*)/;
const CLOSE_FENCE = /^\s*```\s*$/;

/**
 * Text outside fences is trimmed and empty runs are dropped. A fence opens on a line starting with ```
 * (lang = the first word after it) and closes on a bare ``` line. An unterminated fence runs to the end.
 */
export function splitBlocks(answer: string): TextBlock[] {
  const blocks: TextBlock[] = [];
  let prose: string[] = [];
  let code: string[] | null = null;
  let lang = '';

  const flushProse = () => {
    const text = prose.join('\n').trim();
    if (text) blocks.push({ kind: 'text', text });
    prose = [];
  };
  const flushCode = () => {
    const text = (code ?? []).join('\n');
    if (text.trim()) blocks.push({ kind: 'code', text, lang });
    code = null;
    lang = '';
  };

  for (const line of answer.split('\n')) {
    if (code === null) {
      const open = OPEN_FENCE.exec(line);
      if (open) {
        flushProse();
        code = [];
        lang = open[1] ?? '';
      } else {
        prose.push(line);
      }
    } else if (CLOSE_FENCE.test(line)) {
      flushCode();
    } else {
      code.push(line);
    }
  }

  if (code !== null) flushCode();
  else flushProse();
  return blocks;
}
