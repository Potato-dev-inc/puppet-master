// @vitest-environment jsdom
// Markdown from a bot is safe to draw: raw HTML shows as text, a link is a real link only when the page gave an opener
// and the address is http(s), the opener gets the address (the page never navigates), and a checklist is drawn as one.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatMarkdown } from './ChatMarkdown';

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

const render = (text: string, onOpenLink?: (url: string) => void) => act(() => root.render(<ChatMarkdown text={text} onOpenLink={onOpenLink} />));

describe('ChatMarkdown, safety', () => {
  it('shows raw HTML as typed text and creates no element from it', () => {
    render('hi <script>window.hacked = 1</script> <img src=x onerror="window.hacked=2"> <b>bold</b>', () => {});
    expect(host.querySelector('script, img, b')).toBeNull();
    expect(host.textContent).toContain('<script>window.hacked = 1</script>');
    expect(host.textContent).toContain('<img src=x onerror="window.hacked=2">');
  });

  it('draws a web link as a link that hands the address to the opener and does not navigate', () => {
    const open = vi.fn();
    render('See [the docs](https://example.com/a?b=1).', open);
    const a = host.querySelector('a') as HTMLAnchorElement;
    expect(a.textContent).toBe('the docs');
    expect(a.getAttribute('rel')).toBe('noopener noreferrer');
    const click = new MouseEvent('click', { bubbles: true, cancelable: true });
    act(() => {
      a.dispatchEvent(click);
    });
    expect(click.defaultPrevented).toBe(true);
    expect(open).toHaveBeenCalledExactlyOnceWith('https://example.com/a?b=1');
  });

  it('never makes a link of javascript:, data: or file: addresses, even with an opener', () => {
    const open = vi.fn();
    render('[a](javascript:alert(1)) [b](data:text/html,x) [c](file:///C:/x) [d](//evil.test)', open);
    expect(host.querySelector('a')).toBeNull();
    expect(host.querySelectorAll('.ck-chat-md-link')).toHaveLength(4);
    expect(open).not.toHaveBeenCalled();
  });

  it('keeps links inert when the page gave no opener', () => {
    render('[the docs](https://example.com)');
    expect(host.querySelector('a')).toBeNull();
    expect(host.querySelector('.ck-chat-md-link')?.getAttribute('title')).toBe('https://example.com');
  });

  it('draws a fenced block with a Copy button', () => {
    render('```ts\nconst a = 1;\n```', () => {});
    expect(host.querySelector('pre code')?.textContent).toBe('const a = 1;');
    expect(host.querySelector('button.ck-chat-md-copy')?.textContent).toBe('Copy');
  });
});

describe('ChatMarkdown, checklists', () => {
  it('draws task items with a done or open box and no bullet', () => {
    render('- [x] Archived 26 newsletters\n- [ ] Reply to Nora');
    const ul = host.querySelector('ul')!;
    expect(ul.classList.contains('ck-chat-md-checklist')).toBe(true);
    const items = [...ul.querySelectorAll('li')];
    expect(items.map((li) => li.getAttribute('data-checked'))).toEqual(['true', 'false']);
    expect(items[0].textContent).toBe('Archived 26 newsletters');
    expect(items[0].querySelector('[role="img"]')?.getAttribute('aria-label')).toBe('Done');
    expect(items[1].querySelector('[role="img"]')?.getAttribute('aria-label')).toBe('Not done');
  });

  it('draws a tick at the start of an item as a done box and keeps the arrow and the bold words', () => {
    render('- ✓ **Archived** → 26 newsletters\n- ✓ **Replied** → 9 threads');
    const items = [...host.querySelectorAll('li.ck-chat-md-task')];
    expect(items).toHaveLength(2);
    expect(items.every((li) => li.getAttribute('data-checked') === 'true')).toBe(true);
    expect(items[0].textContent).toBe('Archived → 26 newsletters');
    expect(items[0].querySelector('strong')?.textContent).toBe('Archived');
  });

  it('leaves a plain list as it was', () => {
    render('- one\n- two');
    expect(host.querySelector('.ck-chat-md-checklist')).toBeNull();
    expect(host.querySelector('.ck-chat-md-box')).toBeNull();
  });
});
