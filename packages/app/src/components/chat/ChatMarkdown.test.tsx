// @vitest-environment jsdom
import { act } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { parseChatMarkdown } from '../../lib/chatMarkdown';
import { groupTurns, type ChatItem } from '../../lib/sessionChat';
import { ChatMarkdown } from './ChatMarkdown';
import { SessionChatView, type SessionChatViewProps } from './SessionChatView';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

/** Renders the agent text into the host and returns the rendered root element. */
function renderText(text: string): HTMLElement {
  act(() => {
    root.render(<ChatMarkdown text={text} />);
  });
  return host.querySelector('.ck-chat-md') as HTMLElement;
}

const all = (el: ParentNode, sel: string) => Array.from(el.querySelectorAll(sel));

describe('ChatMarkdown blocks', () => {
  it('makes paragraphs and keeps single line breaks', () => {
    const el = renderText('first line\n\nsecond\nthird');
    const paras = all(el, 'p');
    expect(paras).toHaveLength(2);
    expect(paras[0]?.textContent).toBe('first line');
    expect(paras[1]?.querySelectorAll('br')).toHaveLength(1);
    expect(paras[1]?.textContent).toBe('secondthird');
  });

  it('renders # to ### as small headings and leaves #### as text', () => {
    const el = renderText('# One\n## Two\n### Three\n#### Four');
    expect(el.querySelector('h1')?.textContent).toBe('One');
    expect(el.querySelector('h2')?.textContent).toBe('Two');
    expect(el.querySelector('h3')?.textContent).toBe('Three');
    expect(el.querySelectorAll('h1, h2, h3')).toHaveLength(3);
    expect(all(el, 'p')[0]?.textContent).toBe('#### Four');
  });

  it('renders bold, italic and inline code, and removes their markers', () => {
    const el = renderText('**Bridge:** the *port* and _this_ with `npm ci`');
    expect(el.querySelector('strong')?.textContent).toBe('Bridge:');
    expect(all(el, 'em').map((e) => e.textContent)).toEqual(['port', 'this']);
    expect(el.querySelector('code')?.textContent).toBe('npm ci');
    expect(el.textContent).toBe('Bridge: the port and this with npm ci');
  });

  it('keeps markers inside inline code as they are', () => {
    const el = renderText('use `**not bold**` here');
    expect(el.querySelector('code')?.textContent).toBe('**not bold**');
    expect(el.querySelector('strong')).toBeNull();
  });

  it('renders a fenced code block with its language and no inline formatting', () => {
    const el = renderText("```ts\nconst a = '**x**';\n```");
    const pre = el.querySelector('pre');
    expect(pre?.getAttribute('data-lang')).toBe('ts');
    expect(pre?.querySelector('code')?.textContent).toBe("const a = '**x**';");
    expect(el.querySelector('strong')).toBeNull();
  });

  it('renders bulleted lists with - * and +, and numbered lists from their first number', () => {
    const el = renderText('- one\n- two\n\n1. first\n2. second\n\n* star\n\n3. three\n4. four');
    const lists = all(el, 'ul, ol');
    expect(lists.map((l) => l.tagName)).toEqual(['UL', 'OL', 'UL', 'OL']);
    expect(all(lists[0] as HTMLElement, 'li').map((li) => li.textContent)).toEqual(['one', 'two']);
    expect(all(lists[1] as HTMLElement, 'li').map((li) => li.textContent)).toEqual(['first', 'second']);
    expect(lists[3]?.getAttribute('start')).toBe('3');
    expect(lists[1]?.getAttribute('start')).toBeNull();
  });

  it('nests one level of list items by indent', () => {
    const el = renderText('- top\n  - child a\n  - child b\n- next');
    const top = el.querySelector('ul') as HTMLElement;
    const items = Array.from(top.children);
    expect(items.map((li) => li.firstChild?.textContent)).toEqual(['top', 'next']);
    const nested = items[0]?.querySelector('ul');
    expect(Array.from(nested?.children ?? []).map((li) => li.textContent)).toEqual(['child a', 'child b']);
  });

  it('renders quotes and rules', () => {
    const el = renderText('above\n\n---\n\n> quoted **text**\n> more\n\nbelow');
    expect(el.querySelectorAll('hr')).toHaveLength(1);
    const quote = el.querySelector('blockquote');
    expect(quote?.querySelector('strong')?.textContent).toBe('text');
    expect(quote?.textContent).toBe('quoted textmore');
  });

  it('renders a GFM table with alignment, and pads short rows', () => {
    const el = renderText('| Name | Size |\n|:--|--:|\n| a.txt | 4 KB |\n| b.txt |');
    const table = el.querySelector('table') as HTMLElement;
    expect(all(table, 'thead th').map((th) => th.textContent)).toEqual(['Name', 'Size']);
    const rows = all(table, 'tbody tr');
    expect(rows).toHaveLength(2);
    expect(all(rows[0] as HTMLElement, 'td')[1]?.className).toBe('ck-chat-md-align-right');
    expect(all(rows[1] as HTMLElement, 'td').map((td) => td.textContent)).toEqual(['b.txt', '']);
    expect(table.parentElement?.className).toBe('ck-chat-md-table-wrap');
  });

  it('shows a link as its text, with the address in title and no anchor', () => {
    const el = renderText('See [the docs](https://example.com/a?b=1) now');
    expect(el.querySelector('a')).toBeNull();
    const link = el.querySelector('.ck-chat-md-link') as HTMLElement;
    expect(link.textContent).toBe('the docs');
    expect(link.getAttribute('title')).toBe('https://example.com/a?b=1');
    expect(el.textContent).toBe('See the docs now');
  });

  it('keeps a backslash-escaped marker as plain text', () => {
    const el = renderText('\\*not italic\\*');
    expect(el.querySelector('em')).toBeNull();
    expect(el.textContent).toBe('*not italic*');
  });
});

describe('ChatMarkdown safety', () => {
  it('shows script and img markup as text and creates no element from it', () => {
    const el = renderText('<script>alert(1)</script> and <img src=x onerror=alert(1)>');
    expect(el.querySelector('script')).toBeNull();
    expect(el.querySelector('img')).toBeNull();
    expect(el.textContent).toBe('<script>alert(1)</script> and <img src=x onerror=alert(1)>');
  });

  it('keeps unclosed markers as plain text', () => {
    const el = renderText('a **bold never closed\n\nuse `tick here\n\n[label](\n\n2 * 3 * 4\n\nsnake_case_name');
    expect(el.querySelector('strong, em, code, a, .ck-chat-md-link')).toBeNull();
    expect(el.textContent).toContain('a **bold never closed');
    expect(el.textContent).toContain('use `tick here');
    expect(el.textContent).toContain('[label](');
    expect(el.textContent).toContain('2 * 3 * 4');
    expect(el.textContent).toContain('snake_case_name');
  });

  it('keeps an unclosed code fence as text', () => {
    const el = renderText('```\nopen fence');
    expect(el.querySelector('pre')).toBeNull();
    expect(el.textContent).toContain('```');
    expect(el.textContent).toContain('open fence');
  });

  it('never throws on random input', () => {
    const alphabet = '*_`[]()#>-|:\n \t\\!<a1.+~';
    let seed = 7;
    const rand = () => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed / 0x7fffffff;
    };
    for (let n = 0; n < 300; n++) {
      let text = '';
      const len = Math.floor(rand() * 120);
      for (let k = 0; k < len; k++) text += alphabet[Math.floor(rand() * alphabet.length)];
      expect(() => renderToStaticMarkup(<ChatMarkdown text={text} />)).not.toThrow();
    }
  });
});

describe('ChatMarkdown performance', () => {
  it('renders a 50 KB text under a reasonable time', () => {
    const chunk = [
      '## Step',
      'Read **the file** and check `src/app.ts` with _care_ or *more care*.',
      '- item one with [a link](https://example.com)',
      '  - nested item',
      '> a quote',
      '```ts',
      'const x = 1;',
      '```',
      '| a | b |',
      '|---|---|',
      '| 1 | 2 |',
      '',
    ].join('\n');
    const text = chunk.repeat(Math.ceil(50_000 / chunk.length));
    expect(text.length).toBeGreaterThanOrEqual(50_000);
    const started = performance.now();
    const html = renderToStaticMarkup(<ChatMarkdown text={text} />);
    const ms = performance.now() - started;
    expect(html).toContain('<strong>the file</strong>');
    expect(ms).toBeLessThan(2000);
  });

  it('keeps adversarial markers under the same bound', () => {
    const text = ('*a [b `c _d '.repeat(8_000)).slice(0, 50_000);
    const started = performance.now();
    renderToStaticMarkup(<ChatMarkdown text={text} />);
    expect(performance.now() - started).toBeLessThan(2000);
    expect(parseChatMarkdown(text).length).toBeGreaterThan(0);
  });
});

describe('SessionChatView markdown source', () => {
  const items = (text: string): ChatItem[] => [
    { kind: 'user', id: '10:0', at: null, text: 'Say **hi**' },
    { kind: 'assistant', id: '20:0', at: null, text, model: null, finalAnswer: true },
  ];

  function renderChat(props: Partial<SessionChatViewProps>) {
    const full: SessionChatViewProps = {
      state: 'ready',
      reason: null,
      turns: groupTurns(items('**Bridge:** ok')).turns,
      model: null,
      cliVersion: null,
      ...props,
    };
    act(() => {
      root.render(<SessionChatView {...full} />);
    });
    return host.querySelector('[data-testid="session-chat"]') as HTMLElement;
  }

  it('shows the agent text from a screen read as it is', () => {
    const chat = renderChat({ source: 'screen' });
    expect(chat.textContent).toContain('**Bridge:** ok');
    expect(chat.querySelector('strong')).toBeNull();
  });

  it('formats the agent text from its session file, and keeps the person message plain', () => {
    const chat = renderChat({ source: 'file', turns: groupTurns(items('**Bridge:** ok')).turns });
    expect(chat.querySelector('.ck-chat-md strong')?.textContent).toBe('Bridge:');
    expect(chat.textContent).toContain('Say **hi**');
    const bubble = chat.querySelector('.ck-chat-bubble');
    expect(bubble?.querySelector('strong')).toBeNull();
  });

  it('formats the agent text read from the agent API the same way, and does not call it a screen read', () => {
    const chat = renderChat({ source: 'api' });
    expect(chat.querySelector('.ck-chat-md strong')?.textContent).toBe('Bridge:');
    expect(chat.querySelector('[data-testid="session-chat-screen-note"]')).toBeNull();
  });
});
