// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FeedItem } from '../../lib/companion';
import { CompanionFeed, clockLabel, FEED_ALL, FEED_FIRST, readFeedMark, writeFeedMark } from './CompanionFeed';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = new Date(2026, 9, 28, 12, 0).getTime();
const at = (day: number, h: number, m: number) => new Date(2026, 9, day, h, m).getTime();

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
  vi.restoreAllMocks();
});

const item = (n: number): FeedItem => ({ id: `f${n}`, atMs: at(28, 8, n % 60), kind: 'reply', text: `Line ${n}` });

describe('clockLabel', () => {
  it('shows the clock for today and the weekday for another day', () => {
    expect(clockLabel(at(28, 8, 5), NOW)).toBe('8:05');
    expect(clockLabel(at(27, 8, 0), NOW)).toBe('Tue 8:00');
  });
});

describe('the last-seen mark', () => {
  it('reads 0 when nothing is stored and keeps the newer mark on write', () => {
    expect(readFeedMark('mochi')).toBe(0);
    writeFeedMark('mochi', 500);
    writeFeedMark('mochi', 300);
    expect(readFeedMark('mochi')).toBe(500);
    expect(readFeedMark('bit')).toBe(0);
  });

  it('a throwing localStorage reads as 0 and a failed write is ignored', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full');
    });
    expect(readFeedMark('mochi')).toBe(0);
    expect(() => writeFeedMark('mochi', 900)).not.toThrow();
  });
});

describe('CompanionFeed', () => {
  function mount(items: FeedItem[], expanded = false, onToggle = vi.fn()) {
    act(() => {
      root.render(<CompanionFeed items={items} expanded={expanded} onToggle={onToggle} now={NOW} />);
    });
    return { onToggle };
  }

  it('draws nothing without items', () => {
    mount([]);
    expect(host.textContent).toBe('');
  });

  it('shows the first five rows with the heading, and Show all when there are more', () => {
    const items = Array.from({ length: 7 }, (_, i) => item(i));
    const { onToggle } = mount(items);
    expect(host.querySelector('h2')?.textContent).toBe('While you were away');
    expect(host.querySelectorAll('li')).toHaveLength(FEED_FIRST);
    const button = host.querySelector<HTMLButtonElement>('.ph-comp-more')!;
    expect(button.textContent).toBe('Show all');
    act(() => button.click());
    expect(onToggle).toHaveBeenCalledOnce();
  });

  it('expanded shows up to fifty rows', () => {
    const items = Array.from({ length: 60 }, (_, i) => item(i));
    mount(items, true);
    expect(host.querySelectorAll('li')).toHaveLength(FEED_ALL);
    expect(host.querySelector('.ph-comp-more')?.textContent).toBe('Show less');
  });

  it('has no Show all button for five rows or fewer', () => {
    mount([item(1), item(2)]);
    expect(host.querySelector('.ph-comp-more')).toBeNull();
  });
});
