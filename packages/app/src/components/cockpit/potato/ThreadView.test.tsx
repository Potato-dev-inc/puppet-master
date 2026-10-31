// @vitest-environment jsdom
// The thread: bubbles (yours filled, the bot's outlined), the typing row, the header, the composer, the cards.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakePotatoTransport, type FakePotatoTransport, type PotatoSettings } from '../../../lib/potato';
import { ThreadView, type ThreadViewProps } from './ThreadView';
import { bot, buttonByText, msg, note, typeInto, you } from './test-helpers';

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

function setup(over: Partial<ThreadViewProps> = {}): { transport: FakePotatoTransport; props: ThreadViewProps } {
  const transport = fakePotatoTransport({ bots: [bot()] });
  const props: ThreadViewProps = {
    bot: bot(),
    messages: [],
    settings: SETTINGS,
    avatar: 'mochi',
    agentLabel: 'Claude Code',
    transport,
    onOpenSettings: () => {},
    onNotify: () => {},
    onChanged: () => {},
    now: new Date(2026, 10, 1, 15, 0).getTime(),
    ...over,
  };
  return { transport, props };
}
const field = () => host.querySelector<HTMLTextAreaElement>('textarea')!;
const press = (key: string, shiftKey = false) => {
  const e = new KeyboardEvent('keydown', { key, shiftKey, bubbles: true, cancelable: true });
  act(() => {
    field().dispatchEvent(e);
  });
  return e;
};

describe('ThreadView', () => {
  it('draws your bubbles filled and the bot bubbles outlined, one bubble for each segment', () => {
    const { props } = setup({ messages: [you('hello'), msg({ text: 'Fish heard: hello' }), msg({ text: 'second segment' })] });
    act(() => root.render(<ThreadView {...props} />));
    expect(host.querySelectorAll('.ck-potato-bubble--you').length).toBe(1);
    expect(host.querySelectorAll('.ck-potato-bubble--bot').length).toBe(2);
    expect(host.querySelector('.ck-potato-bubble--you')?.textContent).toBe('hello');
  });

  it('shows the note rows in the middle, like Now talking as Potato.', () => {
    const { props } = setup({ messages: [note('Now talking as Potato.')] });
    act(() => root.render(<ThreadView {...props} />));
    expect(host.querySelector('.ck-potato-note')?.textContent).toBe('Now talking as Potato.');
  });

  it('shows Fish is typing… only while the bot types', () => {
    const { props } = setup();
    act(() => root.render(<ThreadView {...props} />));
    expect(host.textContent).not.toContain('is typing');
    act(() => root.render(<ThreadView {...props} bot={bot({ typing: true })} />));
    expect(host.textContent).toContain('Fish is typing…');
  });

  it('shows the header: name, brain, mood text, voice switch and Stop while it runs', () => {
    const { props } = setup({ bot: bot({ mood: { mood: 'genki', intensity: 3, energy: 60, hunger: 5 } }) });
    act(() => root.render(<ThreadView {...props} />));
    expect(host.querySelector('h1')?.textContent).toBe('Fish');
    expect(host.textContent).toContain('DeepSeek · Running');
    expect(host.textContent).toContain('Genki · energy 60');
    expect([...host.querySelectorAll('[role="tab"]')].map((t) => t.textContent)).toEqual(['Fish', 'Potato']);
    expect(buttonByText(host, 'Stop')).toBeDefined();
    expect(buttonByText(host, 'Start')).toBeUndefined();
  });

  it('offers Start while the brain rests, and Start asks the app to start it', async () => {
    const { props, transport } = setup({ bot: bot({ brainState: 'idle' }) });
    await act(async () => root.render(<ThreadView {...props} />));
    await act(async () => buttonByText(host, 'Start')!.click());
    expect(transport.calls.map((c) => c.method)).toContain('startBot');
  });

  it('switching the voice asks the app for the other pack', async () => {
    const { props, transport } = setup();
    await act(async () => root.render(<ThreadView {...props} />));
    await act(async () => buttonByText(host, 'Potato')!.click());
    expect(transport.calls.find((c) => c.method === 'setPersona')?.args).toEqual(['fish', 'potato']);
  });

  it('puts the name in the message box and sends on Enter, then clears the box', async () => {
    const { props, transport } = setup();
    const onChanged = vi.fn();
    await act(async () => root.render(<ThreadView {...props} onChanged={onChanged} />));
    expect(field().placeholder).toBe('Message Fish');
    typeInto(field(), 'hello');
    await act(async () => {
      press('Enter');
    });
    expect(transport.calls.find((c) => c.method === 'send')?.args).toEqual(['fish', 'hello']);
    expect(field().value).toBe('');
    expect(onChanged).toHaveBeenCalled();
  });

  it('adds a new line on Shift+Enter and sends nothing', async () => {
    const { props, transport } = setup();
    await act(async () => root.render(<ThreadView {...props} />));
    typeInto(field(), 'one');
    const e = press('Enter', true);
    expect(e.defaultPrevented).toBe(false);
    expect(transport.calls.some((c) => c.method === 'send')).toBe(false);
  });

  it('sends nothing for an empty box and disables Send', async () => {
    const { props, transport } = setup();
    await act(async () => root.render(<ThreadView {...props} />));
    expect(buttonByText(host, 'Send')!.disabled).toBe(true);
    typeInto(field(), '   ');
    await act(async () => {
      press('Enter');
    });
    expect(transport.calls.some((c) => c.method === 'send')).toBe(false);
  });

  it('gives the text back and says why when the send fails', async () => {
    const { props, transport } = setup();
    const onNotify = vi.fn();
    vi.spyOn(transport, 'send').mockRejectedValue(new Error('POTATO_RATE: That is too fast. Wait a moment, then try again.'));
    await act(async () => root.render(<ThreadView {...props} onNotify={onNotify} />));
    typeInto(field(), 'hello');
    await act(async () => {
      press('Enter');
    });
    expect(onNotify).toHaveBeenCalledWith('That is too fast. Wait a moment, then try again.');
    expect(field().value).toBe('hello');
  });

  it('turns the box off when Potato Bot is off', async () => {
    const { props } = setup({ bot: bot({ brainState: 'off' }) });
    await act(async () => root.render(<ThreadView {...props} />));
    expect(field().disabled).toBe(true);
  });

  it('shows a helper card in the thread with its three buttons; Not now goes to the app', async () => {
    const card = msg({ text: 'fix the build', kind: 'helper_card', requestId: 'r1', project: 'demo-a' });
    const { props, transport } = setup({ messages: [card] });
    await act(async () => root.render(<ThreadView {...props} />));
    expect(host.textContent).toContain('Fish wants a helper: fix the build');
    await act(async () => buttonByText(host, 'Not now')!.click());
    expect(transport.calls.find((c) => c.method === 'helperDecline')?.args).toEqual(['fish', 'r1']);
    expect(transport.calls.some((c) => c.method === 'helperStart')).toBe(false);
  });

  it('Start sends the card id and the chosen folder name to the app, and the page reads again', async () => {
    const card = msg({ text: 'fix the build', kind: 'helper_card', requestId: 'r1', project: 'demo-a' });
    const { props, transport } = setup({ messages: [card] });
    const onChanged = vi.fn();
    await act(async () => root.render(<ThreadView {...props} onChanged={onChanged} />));
    await act(async () => buttonByText(host, 'Change folder')!.click());
    await act(async () => buttonByText(host, 'demo-b')!.click());
    await act(async () => buttonByText(host, 'Start')!.click());
    expect(transport.calls.find((c) => c.method === 'helperFolders')?.args).toEqual(['fish']);
    expect(transport.calls.find((c) => c.method === 'helperStart')?.args).toEqual(['fish', 'r1', 'demo-b']);
    expect(onChanged).toHaveBeenCalled();
  });

  it('a finished helper row has an Open button for its pane, when the pane is still there', async () => {
    const row = msg({ text: 'Helper finished in demo-a.', kind: 'helper_result', requestId: 'r1', paneId: 'pane-9', from: { kind: 'note' } });
    const { props } = setup({ messages: [row] });
    const onOpenPane = vi.fn();
    await act(async () => root.render(<ThreadView {...props} onOpenPane={onOpenPane} paneExists={() => true} />));
    expect(host.querySelector('.ck-potato-note')?.textContent).toBe('Helper finished in demo-a. Open');
    await act(async () => buttonByText(host, 'Open')!.click());
    expect(onOpenPane).toHaveBeenCalledWith('pane-9');
    await act(async () => root.render(<ThreadView {...props} onOpenPane={onOpenPane} paneExists={() => false} />));
    expect(buttonByText(host, 'Open')).toBeUndefined();
    expect(host.querySelector('.ck-potato-note')?.textContent).toBe('Helper finished in demo-a.');
  });

  it('shows the brain card above the box when there is no key', async () => {
    const { props } = setup({ bot: bot({ brainState: 'needs_key' }) });
    await act(async () => root.render(<ThreadView {...props} />));
    expect(host.textContent).toContain('Add your DeepSeek key in Settings to talk with Fish.');
  });

  it('through my agents: no mood, no voice, no composer, and the plain line', async () => {
    const onOpenCompanion = vi.fn();
    const { props } = setup({ bot: bot({ brainState: 'agents', mood: { mood: 'genki', intensity: 1, energy: 1, hunger: 1 } }), settings: { ...SETTINGS, brain: 'agents' }, onOpenCompanion });
    await act(async () => root.render(<ThreadView {...props} />));
    expect(host.textContent).toContain('Mood and personas need the DeepSeek or OpenCode Go brain.');
    expect(host.querySelector('[role="tab"]')).toBeNull();
    expect(host.querySelector('textarea')).toBeNull();
    expect(host.textContent).not.toContain('energy');
    await act(async () => buttonByText(host, 'Open Fish')!.click());
    expect(onOpenCompanion).toHaveBeenCalledWith('fish');
  });
});
