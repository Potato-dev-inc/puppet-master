// @vitest-environment jsdom
// The Computer panel inside a bot's thread: there only when the page was given a screen transport, and only while the
// Labs switches are on; never in the "Through my agents" view.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakePotatoTransport, type PotatoSettings } from '../../../lib/potato';
import { fakeScreenTransport } from '../../../lib/potatoScreen';
import { ThreadView, type ThreadViewProps } from './ThreadView';
import { bot } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

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

const SETTINGS: PotatoSettings = { v: 1, brain: 'deepseek', defaultPersona: 'fish', dailyLimitCents: 50 };
const props = (over: Partial<ThreadViewProps> = {}): ThreadViewProps => ({
  bot: bot(),
  messages: [],
  settings: SETTINGS,
  avatar: 'mochi',
  agentLabel: 'Claude Code',
  transport: fakePotatoTransport({ bots: [bot()] }),
  onOpenSettings: () => {},
  onNotify: () => {},
  onChanged: () => {},
  now: new Date(2026, 10, 1, 15, 0).getTime(),
  ...over,
});
const render = async (p: ThreadViewProps) => {
  await act(async () => root.render(<ThreadView {...p} />));
  await act(async () => {});
};

describe('ThreadView with the Computer', () => {
  it('shows the panel under the header when the screen is on', async () => {
    await render(props({ screen: fakeScreenTransport({ phase: 'bot', bot: 'fish' }) }));
    const panel = host.querySelector('.ck-potato-computer');
    expect(panel).toBeTruthy();
    expect(panel!.textContent).toContain('Fish is using it');
    // after the header, before the messages
    const order = [...host.querySelectorAll('header.ck-potato-head, section.ck-potato-computer, ol[aria-label="Messages"]')].map((el) => el.className.split(' ')[0] || el.tagName);
    expect(order).toEqual(['ck-potato-head', 'ck-potato-computer', 'ck-potato-rows']);
  });

  it('has no panel when the screen is off, or when the page has no screen transport', async () => {
    await render(props({ screen: fakeScreenTransport({ enabled: false }) }));
    expect(host.querySelector('.ck-potato-computer')).toBeNull();
    await render(props());
    expect(host.querySelector('.ck-potato-computer')).toBeNull();
  });

  it('has no panel in the "Through my agents" view', async () => {
    const screen = fakeScreenTransport();
    await render(props({ settings: { ...SETTINGS, brain: 'agents' }, bot: bot({ brainState: 'agents' }), screen }));
    expect(host.querySelector('.ck-potato-computer')).toBeNull();
    expect(screen.calls).toHaveLength(0);
  });
});
