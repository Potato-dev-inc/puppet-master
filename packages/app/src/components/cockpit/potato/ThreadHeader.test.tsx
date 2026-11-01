// @vitest-environment jsdom
// The bot header's daily spend: a line past half the cap, and past 80% a line with "Raise limit" that opens the
// daily-limit row in Settings. Costs are estimates, so the words say "about".
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const openSettingsAt = vi.hoisted(() => vi.fn());
vi.mock('../../../lib/openSettingsAt', () => ({ openSettingsAt }));

import { ThreadHeader, type ThreadHeaderProps } from './ThreadHeader';
import { bot } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// The segmented control in the header measures itself; jsdom has no ResizeObserver.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

const CENT = 10_000_000;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  openSettingsAt.mockClear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function render(over: Partial<ThreadHeaderProps['bot']>) {
  const props: ThreadHeaderProps = {
    bot: bot({ limitCents: 100, ...over }),
    avatar: 'mochi',
    brain: 'deepseek',
    through: false,
    onPersona: vi.fn(),
    onStop: vi.fn(),
    onStart: vi.fn(),
  };
  act(() => root.render(<ThreadHeader {...props} />));
  return host;
}

describe('ThreadHeader today line', () => {
  it('stays hidden at or under half the cap', () => {
    expect(render({ usedTodayNano: 50 * CENT })?.textContent).not.toContain('Today');
  });

  it('shows today against the cap past half', () => {
    const text = render({ usedTodayNano: 60 * CENT })?.querySelector('.ck-potato-today')?.textContent;
    expect(text).toBe('Today about 60¢ of $1');
  });

  it('uses dollars with cents when the cap is not whole dollars', () => {
    const text = render({ limitCents: 250, usedTodayNano: 150 * CENT })?.querySelector('.ck-potato-today')?.textContent;
    expect(text).toBe('Today about 150¢ of $2.50');
  });

  it('has no 80% line below 80%', () => {
    expect(render({ usedTodayNano: 79 * CENT })?.querySelector('.ck-potato-limit-line')).toBeNull();
  });
});

describe('ThreadHeader 80% line', () => {
  it('appears at 80% with a Raise limit button for the daily-limit row', () => {
    const host2 = render({ usedTodayNano: 80 * CENT });
    const line = host2.querySelector('.ck-potato-limit-line');
    expect(line?.textContent).toContain("has used most of today's limit.");
    const button = [...(line?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Raise limit');
    expect(button).toBeDefined();
    act(() => button!.click());
    expect(openSettingsAt).toHaveBeenCalledWith('potato', 'potato-limit');
  });
});
