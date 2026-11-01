// @vitest-environment jsdom
// A new bot's first run in the thread: the scripted first message and the choice card. It shows on an empty bot only,
// speaks in the bot's persona, its buttons send their text as the first message, it is gone after that message and it
// never comes back for that bot. "Use the computer" follows the sandbox Labs switch.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakePotatoTransport, type FakePotatoTransport, type PotatoBotView, type PotatoSettings } from '../../../lib/potato';
import { fakeScreenTransport, type FakeScreenTransport } from '../../../lib/potatoScreen';
import { ThreadView, type ThreadViewProps } from './ThreadView';
import { bot, buttonByText, you } from './test-helpers';

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
/** A bot nobody has written to: no last-message time. */
const NEW: Partial<PotatoBotView> = { lastAt: null };

function setup(over: Partial<ThreadViewProps> = {}, botOver: Partial<PotatoBotView> = NEW) {
  const transport: FakePotatoTransport = fakePotatoTransport({ bots: [bot(botOver)] });
  const props: ThreadViewProps = {
    bot: bot(botOver),
    messages: [],
    settings: SETTINGS,
    avatar: 'mochi',
    agentLabel: 'Claude',
    transport,
    onOpenSettings: () => {},
    onNotify: () => {},
    onChanged: () => {},
    now: new Date(2026, 10, 1, 15, 0).getTime(),
    ...over,
  };
  return { transport, props };
}

/** Lets the sandbox state read finish. */
const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

const greeting = () => host.querySelector<HTMLElement>('[aria-label^="Start with "]');
const choice = (label: string) => buttonByText(host, label);

describe('ThreadView first run', () => {
  it('shows on an empty bot only', async () => {
    const { props } = setup();
    await act(async () => root.render(<ThreadView {...props} />));
    expect(greeting()).not.toBeNull();

    // A bot with a message in its thread (the thread may still be loading): no first run.
    await act(async () => root.render(<ThreadView {...props} messages={[you('hi')]} />));
    expect(greeting()).toBeNull();
    await act(async () => root.render(<ThreadView {...props} bot={bot({ lastAt: 1_700_000_000_000 })} messages={[]} />));
    expect(greeting()).toBeNull();
  });

  it('says it in the persona of the bot, and names the bot', async () => {
    const fish = setup({}, { ...NEW, talkPersona: 'fish', name: 'Chief' });
    await act(async () => root.render(<ThreadView {...fish.props} />));
    expect(greeting()!.textContent).toContain("I'm Chief");
    expect(greeting()!.textContent).toContain('a little lazy');

    window.localStorage.clear();
    const potato = setup({}, { ...NEW, talkPersona: 'potato', name: 'Spud' });
    await act(async () => root.render(<ThreadView {...potato.props} key="spud" />));
    expect(greeting()!.textContent).toContain("I'm Spud, a little potato king");
    expect(greeting()!.textContent).not.toContain('lazy');
  });

  it.each([
    ['Just chat', "Let's just chat."],
    ['Help with a project', 'I want help with a project.'],
    ['What can you do?', 'What can you do?'],
  ])('the "%s" button sends its text as the first message', async (label, text) => {
    const screen = fakeScreenTransport({ enabled: true });
    const { props, transport } = setup({ screen });
    const onChanged = vi.fn();
    await act(async () => root.render(<ThreadView {...props} onChanged={onChanged} />));
    await settle();
    await act(async () => choice(label)!.click());
    expect(transport.calls.find((c) => c.method === 'send')?.args).toEqual(['fish', text]);
    expect(onChanged).toHaveBeenCalled();
  });

  it('the "Use the computer" button sends its text as the first message while the sandbox is on', async () => {
    const screen = fakeScreenTransport({ enabled: true });
    const { props, transport } = setup({ screen });
    await act(async () => root.render(<ThreadView {...props} />));
    await settle();
    await act(async () => choice('Use the computer')!.click());
    expect(transport.calls.find((c) => c.method === 'send')?.args).toEqual(['fish', 'Use your computer to help me.']);
  });

  it('is gone after the first real message, and does not come back once the bot has been used', async () => {
    const { props } = setup();
    await act(async () => root.render(<ThreadView {...props} />));
    expect(greeting()).not.toBeNull();

    const used = bot({ lastAt: 1_700_000_000_000 });
    await act(async () => root.render(<ThreadView {...props} bot={used} messages={[you("Let's just chat.")]} />));
    expect(greeting()).toBeNull();
    expect(window.localStorage.getItem('pm.potato.firstrun.fish')).toBe('1');

    // The history is cleared later (or the thread is opened again): still never shown for this bot.
    await act(async () => root.render(<ThreadView {...props} bot={bot(NEW)} messages={[]} />));
    expect(greeting()).toBeNull();
    act(() => root.unmount());
    root = createRoot(host);
    await act(async () => root.render(<ThreadView {...props} bot={bot(NEW)} messages={[]} />));
    expect(greeting()).toBeNull();
  });

  it('leaves "Use the computer" out while the sandbox is off, and the line about it too', async () => {
    const off: FakeScreenTransport = fakeScreenTransport({ enabled: false });
    const { props } = setup({ screen: off });
    await act(async () => root.render(<ThreadView {...props} />));
    await settle();
    expect(choice('Use the computer')).toBeUndefined();
    expect(choice('Just chat')).toBeDefined();
    expect(greeting()!.textContent).not.toMatch(/sandbox/);
  });

  it('shows "Use the computer" while the sandbox is on, and when there is no sandbox at all it stays out', async () => {
    const on = setup({ screen: fakeScreenTransport({ enabled: true }) });
    await act(async () => root.render(<ThreadView {...on.props} />));
    await settle();
    expect(choice('Use the computer')).toBeDefined();
    expect(greeting()!.textContent).toMatch(/own computer \(a sandbox\)/);

    const none = setup();
    await act(async () => root.render(<ThreadView {...none.props} key="none" />));
    await settle();
    expect(choice('Use the computer')).toBeUndefined();
  });
});

describe('ThreadView first run, one empty-thread message and the composer commands', () => {
  it('shows the first message and not "Say hello" for a new bot', async () => {
    const { props } = setup();
    await act(async () => root.render(<ThreadView {...props} />));
    expect(greeting()).not.toBeNull();
    expect(host.textContent).not.toContain('Say hello to');
  });

  it('falls back to "Say hello" once the first run is used up and the thread is still empty', async () => {
    window.localStorage.clear();
    const { props } = setup();
    await act(async () => root.render(<ThreadView {...props} />));
    await act(async () => root.render(<ThreadView {...props} messages={[you('hi')]} />));
    await act(async () => root.render(<ThreadView {...props} messages={[]} />));
    expect(greeting()).toBeNull();
    expect(host.textContent).toContain('Say hello to');
  });

  it('/computer opens the bot panel on Computer, and /new starts the new-bot flow', async () => {
    const onNewBot = vi.fn();
    const onOpenComputer = vi.fn();
    const { props } = setup({ onNewBot, panel: { open: false, computerShown: false, onToggle: () => {}, onOpenComputer } });
    await act(async () => root.render(<ThreadView {...props} />));
    const field = host.querySelector<HTMLTextAreaElement>('textarea')!;
    const enter = () => act(async () => void field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })));
    const typeText = async (text: string) => {
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(field, text);
        field.dispatchEvent(new Event('input', { bubbles: true }));
      });
    };
    await typeText('/computer');
    await enter();
    expect(onOpenComputer).toHaveBeenCalledTimes(1);
    await typeText('/new');
    await enter();
    expect(onNewBot).toHaveBeenCalledTimes(1);
  });
});
