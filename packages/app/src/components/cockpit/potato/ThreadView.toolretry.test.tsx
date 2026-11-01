// @vitest-environment jsdom
// A failed last step in Fish's latest reply: "Try again" sends the retry words as the person. After two failures of the
// same step in a row (two replies), "Take control" opens the computer instead.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakePotatoTransport, type PotatoSettings } from '../../../lib/potato';
import { ThreadView, type ThreadViewProps } from './ThreadView';
import { bot, msg } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  window.localStorage.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const SETTINGS: PotatoSettings = { v: 1, brain: 'deepseek', defaultPersona: 'fish', dailyLimitCents: 50 };
const failed = (id: string) => msg({ id, text: 'Fish used browser_navigate: failed LEASE_LOST' });
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === text) ?? null;

function setup(messages: ReturnType<typeof msg>[], onOpenComputer = vi.fn()) {
  const transport = fakePotatoTransport({ bots: [bot()] });
  const props: ThreadViewProps = {
    bot: bot(),
    messages,
    settings: SETTINGS,
    avatar: 'mochi',
    agentLabel: 'Claude',
    transport,
    onOpenSettings: () => {},
    onNotify: () => {},
    onChanged: () => {},
    now: new Date(2026, 10, 1, 15, 0).getTime(),
    panel: { open: false, computerShown: false, onToggle: () => {}, onOpenComputer },
  };
  return { transport, onOpenComputer, props };
}

describe('ThreadView: a failed step offers Try again or Take control', () => {
  it('shows Try again on a failed last step, and clicking it sends the retry words', async () => {
    const { transport, props } = setup([failed('a')]);
    await act(async () => root.render(<ThreadView {...props} />));
    expect(host.textContent).toContain("Couldn't open the page");
    await act(async () => button('Try again')!.click());
    expect(transport.calls.find((c) => c.method === 'send')?.args).toEqual(['fish', 'Please try that again.']);
  });

  it('shows Take control, not Try again, after the same step failed in two replies in a row', async () => {
    const { transport, onOpenComputer, props } = setup([failed('a'), msg({ id: 'b', text: 'That did not work.' }), failed('c')]);
    await act(async () => root.render(<ThreadView {...props} />));
    expect(button('Try again')).toBeNull();
    await act(async () => button('Take control')!.click());
    expect(onOpenComputer).toHaveBeenCalledTimes(1);
    expect(transport.calls.some((c) => c.method === 'send')).toBe(false);
  });

  it('shows nothing when the last step worked', async () => {
    const { props } = setup([msg({ id: 'a', text: 'Fish used browser_navigate: ok, saw Fake desktop' })]);
    await act(async () => root.render(<ThreadView {...props} />));
    expect(button('Try again')).toBeNull();
    expect(button('Take control')).toBeNull();
  });
});
