// @vitest-environment jsdom
// The thread and the computer: the thread no longer holds the computer (the bot panel does), but it must say when the
// bot needs the person there. The "Needs you" banner shows while this bot is the one asking and the Computer tab is not
// on screen; its button opens the tab. Never in the "Through my agents" view.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
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
  agentLabel: 'Claude',
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
const panel = (over: Partial<NonNullable<ThreadViewProps['panel']>> = {}) => ({ open: false, computerShown: false, onToggle: () => {}, onOpenComputer: () => {}, ...over });
const asking = () => fakeScreenTransport({ phase: 'needs_you', bot: 'fish', cardId: 'needs-1', reason: 'The site wants a code' });
const banner = () => host.querySelector('.ck-potato-banner');

describe('the computer in the thread', () => {
  it('is not in the thread any more: no Computer panel, whatever the screen says', async () => {
    await render(props({ screen: fakeScreenTransport({ phase: 'bot', bot: 'fish' }), panel: panel() }));
    expect(host.querySelector('.ck-potato-computer')).toBeNull();
    expect(banner()).toBeNull();
  });

  it('has a button for the bot panel in the header, which says what it does', async () => {
    const onToggle = vi.fn();
    await render(props({ panel: panel({ onToggle }) }));
    const button = host.querySelector<HTMLButtonElement>('header .ck-potato-panel-toggle')!;
    expect(button.getAttribute('aria-label')).toBe('Show bot panel');
    expect(button.getAttribute('aria-pressed')).toBe('false');
    await act(async () => button.click());
    expect(onToggle).toHaveBeenCalled();
    await render(props({ panel: panel({ open: true, onToggle }) }));
    const open = host.querySelector('header .ck-potato-panel-toggle')!;
    expect(open.getAttribute('aria-label')).toBe('Hide bot panel');
    expect(open.getAttribute('aria-pressed')).toBe('true');
  });

  it('has no panel button when the page has no panel', async () => {
    await render(props());
    expect(host.querySelector('.ck-potato-panel-toggle')).toBeNull();
  });
});

describe('the "Needs you" banner', () => {
  it('shows while the panel is closed, with the reason and an Open computer button', async () => {
    const onOpenComputer = vi.fn();
    await render(props({ screen: asking(), panel: panel({ onOpenComputer }) }));
    expect(banner()).not.toBeNull();
    expect(banner()!.textContent).toContain('Needs you');
    expect(banner()!.textContent).toContain('Fish needs you on the computer.');
    expect(banner()!.textContent).toContain('The site wants a code');
    // it sits between the header and the messages
    const order = [...host.querySelectorAll('header.ck-potato-head, .ck-potato-banner, ol[aria-label="Messages"]')].map((el) => el.className.split(' ')[0] || el.tagName);
    expect(order).toEqual(['ck-potato-head', 'ck-potato-banner', 'ck-potato-rows']);
    const button = [...banner()!.querySelectorAll('button')].find((b) => b.textContent === 'Open computer')!;
    await act(async () => button.click());
    expect(onOpenComputer).toHaveBeenCalled();
  });

  it('shows while the panel is open on another tab, and goes away when the Computer tab is showing', async () => {
    await render(props({ screen: asking(), panel: panel({ open: true, computerShown: false }) }));
    expect(banner()).not.toBeNull();
    await render(props({ screen: asking(), panel: panel({ open: true, computerShown: true }) }));
    expect(banner()).toBeNull();
  });

  it('comes and goes as the screen moves (the app says so with an event)', async () => {
    const screen = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await render(props({ screen, panel: panel() }));
    expect(banner()).toBeNull();
    await act(async () => screen.set({ phase: 'needs_you', bot: 'fish', cardId: 'needs-2', reason: 'Look' }));
    expect(banner()).not.toBeNull();
    await act(async () => screen.set({ phase: 'bot', cardId: null, reason: null }));
    expect(banner()).toBeNull();
  });

  it('is for this bot only, and only while the Labs switches are on', async () => {
    await render(props({ screen: fakeScreenTransport({ phase: 'needs_you', bot: 'someone-else', cardId: 'n', reason: 'x' }), panel: panel() }));
    expect(banner()).toBeNull();
    await render(props({ screen: fakeScreenTransport({ enabled: false, phase: 'needs_you', bot: 'fish', cardId: 'n', reason: 'x' }), panel: panel() }));
    expect(banner()).toBeNull();
  });

  it('is not drawn without a panel to open, or in the "Through my agents" view (which never asks for the screen)', async () => {
    await render(props({ screen: asking() }));
    expect(banner()).toBeNull();
    const screen = asking();
    await render(props({ settings: { ...SETTINGS, brain: 'agents' }, bot: bot({ brainState: 'agents' }), screen, panel: panel() }));
    expect(banner()).toBeNull();
    expect(screen.calls).toHaveLength(0);
  });
});
