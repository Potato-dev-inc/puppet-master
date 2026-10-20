// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CanaryApi, CanaryStatus } from '../../lib/canary';
import { CanaryNotices } from './CanaryNotices';

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

const status = (texts: string[]): CanaryStatus => ({
  agents: [],
  auto_check: true,
  captures: 0,
  notices: texts.map((text, i) => ({ id: `n${i}`, agent: 'codex', text, at_ms: 1, dismissed: false, kind: 'changed' as const })),
});

function fakeApi(s: CanaryStatus): CanaryApi {
  return {
    status: () => Promise.resolve(s),
    check: vi.fn(),
    setAuto: vi.fn(),
    dismissNotice: vi.fn(() => Promise.resolve()),
    openCaptures: vi.fn(),
    onChanged: () => Promise.resolve(() => {}),
  } as unknown as CanaryApi;
}

describe('version notes in the Inbox', () => {
  it('renders nothing when there are none', async () => {
    await act(async () => root.render(<CanaryNotices api={fakeApi(status([]))} />));
    expect(host.innerHTML).toBe('');
  });

  it('is the same card as every other: the calm glyph, the text as the title, one Dismiss in the action row, no number', async () => {
    const api = fakeApi(status(['Codex 0.162.1 changed its screens. Puppet Master will ask you before acting.']));
    await act(async () => root.render(<CanaryNotices api={api} />));
    const card = host.querySelector('[data-testid="canary-notice"]')!;
    expect(card.matches('[data-ib-card][data-kind="note"]')).toBe(true);
    expect(card.querySelector('.ib-title')?.textContent).toBe('Codex 0.162.1 changed its screens. Puppet Master will ask you before acting.');
    // the calm glyph (idle corners), hidden from screen readers; never the X of an error
    expect(card.querySelector('.ib-glyph [role="img"]')?.getAttribute('aria-label')).toBe('Idle');
    expect(card.querySelector('.ib-age')).toBeNull();
    expect(Array.from(card.querySelectorAll('button')).map((b) => b.textContent)).toEqual(['Dismiss']);
    expect(card.querySelectorAll('.ib-actions')).toHaveLength(1);
    await act(async () => card.querySelector('button')!.click());
    expect(api.dismissNotice).toHaveBeenCalledWith('n0');
  });

  it('shows a good result as the same calm card, marked as good, with the same Dismiss', async () => {
    const good: CanaryStatus = {
      ...status([]),
      notices: [
        { id: 'canary:claude:2.4.0:ok', agent: 'claude', text: 'Claude Code 2.3.0 \u2192 2.4.0: checked, all good.', at_ms: 1, dismissed: false, kind: 'good' },
      ],
    };
    const api = fakeApi(good);
    await act(async () => root.render(<CanaryNotices api={api} />));
    const card = host.querySelector('[data-testid="canary-notice"]')!;
    expect(card.getAttribute('data-result')).toBe('good');
    expect(card.querySelector('.ib-title')?.textContent).toBe('Claude Code 2.3.0 \u2192 2.4.0: checked, all good.');
    expect(Array.from(card.querySelectorAll('button')).map((b) => b.textContent)).toEqual(['Dismiss']);
    await act(async () => card.querySelector('button')!.click());
    expect(api.dismissNotice).toHaveBeenCalledWith('canary:claude:2.4.0:ok');
  });

  it('marks a changed-screens card as changed', async () => {
    await act(async () => root.render(<CanaryNotices api={fakeApi(status(['x']))} />));
    expect(host.querySelector('[data-testid="canary-notice"]')?.getAttribute('data-result')).toBe('changed');
  });

  it('spaces several notes like every other list of cards', async () => {
    await act(async () => root.render(<CanaryNotices api={fakeApi(status(['one', 'two']))} />));
    expect(host.querySelectorAll('ul.ib-list > li')).toHaveLength(2);
  });
});
