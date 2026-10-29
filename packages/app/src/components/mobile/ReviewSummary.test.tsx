// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PhoneReviewItem, PhoneReviewView, ReviewPhoneApi } from '../../lib/mobile-review';
import { ReviewSummary, REVIEW_ROWS } from './ReviewSummary';

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
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** One row as `GET /review-desk` returns it (words and numbers only). */
function item(overrides: Partial<PhoneReviewItem> = {}): PhoneReviewItem {
  return {
    title: 'Test files changed · cart.test.ts',
    levelWord: 'High risk',
    agentLabel: 'Fixture agent',
    whereWord: 'Copy',
    files: 2,
    added: 9,
    deleted: 1,
    ...overrides,
  };
}

function desk(overrides: Partial<PhoneReviewView> = {}): PhoneReviewView {
  return { count: 4, high: 1, items: [item(), item({ levelWord: 'Check first', title: 'Second', agentLabel: 'Other agent' })], ...overrides };
}

/** An api whose reads answer `views` in order (the last one repeats). */
function apiOf(...views: PhoneReviewView[]): ReviewPhoneApi & { read: ReturnType<typeof vi.fn> } {
  let i = 0;
  const read = vi.fn(async () => {
    const next = views[Math.min(i, views.length - 1)];
    i += 1;
    return next;
  });
  return { read } as ReviewPhoneApi & { read: ReturnType<typeof vi.fn> };
}

/** Lets the pending read resolve and React commit it. */
async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function mount(api: ReviewPhoneApi | null, refreshKey = 0): Promise<void> {
  await act(async () => {
    root.render(<ReviewSummary api={api} refreshKey={refreshKey} />);
  });
  await flush();
}

describe('ReviewSummary', () => {
  it('is hidden when nothing is waiting', async () => {
    await mount(apiOf(desk({ count: 0, high: 0, items: [] })));
    expect(host.querySelector('[aria-label="Review Desk"]')).toBeNull();
    expect(host.textContent).toBe('');
  });

  it('is hidden with no api and before the first read answers', async () => {
    await mount(null);
    expect(host.textContent).toBe('');
  });

  it('shows the summary line in words and numbers', async () => {
    await mount(apiOf(desk()));
    expect(host.querySelector('.ph-card-q')?.textContent).toBe('4 to review · 1 high risk');
  });

  it('leaves the high-risk part out when nothing is high risk', async () => {
    await mount(apiOf(desk({ count: 2, high: 0 })));
    expect(host.querySelector('.ph-card-q')?.textContent).toBe('2 to review');
  });

  it('shows at most five rows, each with a mark, a title and the agent', async () => {
    const many = Array.from({ length: 7 }, (_, i) => item({ title: `Row ${i}`, levelWord: 'Low risk' }));
    await mount(apiOf(desk({ count: 7, high: 0, items: many })));
    const rows = host.querySelectorAll('ul.ph-group > li');
    expect(REVIEW_ROWS).toBe(5);
    expect(rows).toHaveLength(5);
    expect(rows[0].querySelector('.ph-glyph')?.textContent).toBe('○');
    expect(rows[0].querySelector('.ph-t')?.textContent).toBe('Row 0');
    expect(rows[0].querySelector('.ph-s')?.textContent).toBe('Fixture agent');
  });

  it('marks each row by its level word', async () => {
    await mount(apiOf(desk()));
    const marks = Array.from(host.querySelectorAll('.ph-glyph')).map((el) => el.textContent);
    expect(marks).toEqual(['●', '◐']);
  });

  it('ends with the computer-only footer and has no buttons or links', async () => {
    await mount(apiOf(desk()));
    expect(host.querySelector('.ph-foot')?.textContent).toBe('Keep and Throw away are on your computer.');
    expect(host.querySelectorAll('button, a, input, select, textarea')).toHaveLength(0);
  });

  it('draws titles and agent labels as text, never as markup', async () => {
    const hostile = '<img src=x onerror="alert(1)"><b>bold</b>';
    await mount(apiOf(desk({ count: 1, high: 0, items: [item({ title: hostile, agentLabel: hostile })] })));
    expect(host.querySelector('img')).toBeNull();
    expect(host.querySelector('b')).toBeNull();
    expect(host.querySelector('.ph-t')?.textContent).toBe(hostile);
  });

  it('reads again on the refresh timer and when the resync key changes, and keeps the last list when a read fails', async () => {
    vi.useFakeTimers();
    const next = desk({ count: 5, high: 2 });
    const api = {
      read: vi
        .fn()
        .mockResolvedValueOnce(desk())
        .mockRejectedValueOnce(new Error('bridge down'))
        .mockResolvedValueOnce(next)
        .mockResolvedValue(next),
    } as unknown as ReviewPhoneApi;
    await mount(api);
    expect(host.querySelector('.ph-card-q')?.textContent).toBe('4 to review · 1 high risk');

    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    await flush();
    // The failed read keeps what was shown.
    expect(host.querySelector('.ph-card-q')?.textContent).toBe('4 to review · 1 high risk');

    await act(async () => {
      root.render(<ReviewSummary api={api} refreshKey={1} />);
    });
    await flush();
    expect(host.querySelector('.ph-card-q')?.textContent).toBe('5 to review · 2 high risk');
    // One read on mount, one on the timer, one on the resync key.
    expect((api.read as unknown as ReturnType<typeof vi.fn>).mock.calls.length).toBe(3);
  });
});
