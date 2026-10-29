import { afterEach, describe, expect, it, vi } from 'vitest';
import { bridgeReviewApi, reviewGlyph, reviewSummaryLine, type PhoneReviewView } from './mobile-review';

afterEach(() => {
  vi.unstubAllGlobals();
});

const view: PhoneReviewView = {
  count: 4,
  high: 1,
  items: [
    { title: 'Test files changed · cart.test.ts', levelWord: 'High risk', agentLabel: 'Fixture agent', whereWord: 'Copy', files: 2, added: 9, deleted: 1 },
  ],
};

describe('bridgeReviewApi', () => {
  const base = 'http://phone.test';

  it('reads GET /review-desk and returns the phone view as it came', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify(view), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    expect(await bridgeReviewApi(base).read()).toEqual(view);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit | undefined];
    expect(url).toBe(`${base}/review-desk`);
    expect(init?.method ?? 'GET').toBe('GET');
  });

  it('throws when the PC refuses the read', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 403 })));
    await expect(bridgeReviewApi(base).read()).rejects.toThrow('403');
  });
});

describe('reviewGlyph', () => {
  it('marks high risk, check first and low risk with the desk marks', () => {
    expect(reviewGlyph('High risk')).toBe('●');
    expect(reviewGlyph('Check first')).toBe('◐');
    expect(reviewGlyph('Low risk')).toBe('○');
  });
});

describe('reviewSummaryLine', () => {
  it('says how many are waiting and how many are high risk', () => {
    expect(reviewSummaryLine({ count: 4, high: 1 })).toBe('4 to review · 1 high risk');
  });

  it('leaves the high-risk part out when none is high risk', () => {
    expect(reviewSummaryLine({ count: 2, high: 0 })).toBe('2 to review');
  });
});
