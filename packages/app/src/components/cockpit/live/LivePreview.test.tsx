// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { LiveItem } from '../../../lib/live-room';
import { LivePreview } from './LivePreview';

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

async function render(items: LiveItem[], hidden = 0) {
  await act(async () => {
    root.render(<LivePreview items={items} hidden={hidden} panes={[{ key: 'p1', label: 'Claude · shop', title: 'shop', state: 'working', line: null }]} />);
  });
}

describe('LivePreview', () => {
  it('renders chat items as plain text, so markup in a message stays text', async () => {
    await render([
      { t: 'user', id: 1, pane: 'p1', text: 'fix <b>this</b> & <img src=x>' },
      { t: 'agent', id: 2, pane: 'p1', text: 'Done. See <a href="https://example.test">here</a>.' },
    ]);
    expect(host.querySelector('b')).toBeNull();
    expect(host.querySelector('img')).toBeNull();
    expect(host.querySelector('a')).toBeNull();
    expect(host.textContent).toContain('fix <b>this</b> & <img src=x>');
    expect(host.textContent).toContain('Done. See <a href="https://example.test">here</a>.');
  });

  it('shows the pane label for each step', async () => {
    await render([{ t: 'note', id: 3, pane: 'p1', text: 'Opened a pull request' }]);
    expect(host.querySelector('.ck-live-src')?.textContent).toBe('Claude · shop');
  });

  it('describes tool steps by name, summary, files and writes, never their input', async () => {
    await render([{ t: 'tool', id: 4, pane: 'p1', name: 'Edit', summary: 'src/app.ts', files: 2, writes: true }]);
    expect(host.querySelector('.ck-live-body')?.textContent).toBe('Edit · src/app.ts · 2 files · writes');
  });

  it('describes results by ok and line count only', async () => {
    await render([
      { t: 'result', id: 5, pane: 'p1', ok: true, lines: 12 },
      { t: 'result', id: 6, pane: 'p1', ok: false, lines: 1 },
    ]);
    const bodies = Array.from(host.querySelectorAll('.ck-live-body')).map((b) => b.textContent);
    expect(bodies).toEqual(['Done · 12 lines', 'Failed · 1 line']);
  });

  it('lists plan tasks with their status', async () => {
    await render([
      {
        t: 'plan',
        id: 7,
        pane: 'p1',
        replace: false,
        tasks: [
          { text: 'Read the code', status: 'done' },
          { text: 'Write the test', status: null },
        ],
      },
    ]);
    const tasks = Array.from(host.querySelectorAll('.ck-live-tasks li')).map((li) => li.textContent);
    expect(tasks).toEqual(['Read the code (done)', 'Write the test']);
  });

  it('says some steps were not shared when some were held back', async () => {
    await render([{ t: 'user', id: 1, pane: 'p1', text: 'hi' }], 3);
    expect(host.querySelector('.ck-live-hidden')?.textContent).toBe('Some steps were not shared');
  });

  it('says nothing is shown yet when there are no items', async () => {
    await render([]);
    expect(host.textContent).toContain('Nothing to show yet');
    expect(host.querySelector('ol')).toBeNull();
  });
});
