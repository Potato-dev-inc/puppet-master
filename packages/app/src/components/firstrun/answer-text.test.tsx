// @vitest-environment jsdom
// The answer never shows markdown marks.
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { Answer, blocks } from './answer-text';

const REAL = 'This is a tiny plant shop. The README says it lists plants.\n\nThe project has three files:\n- **`README.md`** is a two-line description.\n- **`package.json`** names the project `tiny-shop`.\n\nI didn\'t change any files.';

describe('answer text', () => {
  it('splits paragraphs and lists', () => {
    expect(blocks(REAL).map((b) => b.kind)).toEqual(['p', 'p', 'ul', 'p']);
    expect(blocks('one\ntwo')).toEqual([{ kind: 'p', text: 'one two' }]);
  });
  it('shows no star and no backtick (the real answer of the live run)', () => {
    const html = renderToStaticMarkup(<Answer text={REAL} />);
    expect(html).not.toContain('*');
    expect(html).not.toContain('`');
    expect(html).toContain('<strong>');
    expect(html).toContain('<li>');
    expect(html).toContain('README.md');
  });
});
