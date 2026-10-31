// @vitest-environment jsdom
// The page: the list next to the thread, the open bot, sending and the typing row driven by the transport's
// change event, the voice switch, and "New bot" turning a new companion into a bot.
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type CompanionView } from '../../../lib/companion';
import { fakePotatoTransport, type FakePotatoTransport, type PotatoState } from '../../../lib/potato';
import { usePotatoState } from '../../../hooks/usePotato';
import { PotatoPage } from './PotatoPage';
import { bot, buttonByText, msg, typeInto, you } from './test-helpers';

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

const companion = (over: Partial<CompanionView> = {}): CompanionView => ({
  id: 'fish',
  name: 'Fish',
  avatar: 'bit',
  persona: 'cheerful',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'fish',
  state: 'idle',
  stateLine: 'Here.',
  paneId: 'companion-fish',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
  ...over,
});

/** The page the way CockpitApp drives it: the state hook feeds the page. */
function Harness({ transport, companions, onOpenBot }: { transport: FakePotatoTransport; companions: CompanionView[]; onOpenBot?: (id: string) => void }) {
  const potato = usePotatoState(transport);
  const [openId, setOpenId] = useState<string | null>(null);
  if (!potato.state) return null;
  return (
    <PotatoPage
      state={potato.state as PotatoState}
      version={potato.version}
      transport={transport}
      companionTransport={fakeCompanionTransport({ companions })}
      companions={companions}
      openBotId={openId}
      onOpenBot={(id) => {
        setOpenId(id);
        onOpenBot?.(id);
      }}
      onChanged={potato.refresh}
      onCompanionsChanged={() => {}}
      onNotify={() => {}}
      onOpenSettings={() => {}}
    />
  );
}

async function mount(transport: FakePotatoTransport, companions: CompanionView[] = [companion()], onOpenBot?: (id: string) => void) {
  await act(async () => root.render(<Harness transport={transport} companions={companions} onOpenBot={onOpenBot} />));
  await act(async () => {});
}

describe('PotatoPage', () => {
  it('opens the first bot, with its thread next to the list', async () => {
    const transport = fakePotatoTransport({ bots: [bot({ lastLine: 'hi' })], messages: { fish: [you('hello'), msg({ text: 'Fish heard: hello' })] } });
    await mount(transport);
    expect(host.querySelector('.ck-potato-list')).not.toBeNull();
    expect(host.querySelector('h1')?.textContent).toBe('Fish');
    expect(host.querySelector('.ck-potato-bubble--bot')?.textContent).toBe('Fish heard: hello');
    expect(host.querySelector('textarea')?.placeholder).toBe('Message Fish');
  });

  it('marks the thread read when it opens with an unread reply', async () => {
    const transport = fakePotatoTransport({ bots: [bot({ unread: 1 })], messages: { fish: [msg({ text: 'hi' })] } });
    await mount(transport);
    expect(transport.calls.some((c) => c.method === 'markRead' && c.args[0] === 'fish')).toBe(true);
  });

  it('sends a message, then shows typing and the reply as the app reports them', async () => {
    const transport = fakePotatoTransport({ bots: [bot()] });
    await mount(transport);
    typeInto(host.querySelector('textarea')!, 'hello');
    await act(async () => {
      host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    });
    await act(async () => {});
    expect(host.querySelector('.ck-potato-bubble--you')?.textContent).toBe('hello');

    await act(async () => transport.patchBot('fish', { typing: true }));
    expect(host.textContent).toContain('Fish is typing…');

    await act(async () => {
      transport.addMessage('fish', msg({ text: 'Fish heard: hello' }));
      transport.patchBot('fish', { typing: false, mood: { mood: 'genki', intensity: 3, energy: 60, hunger: 5 } });
    });
    await act(async () => {});
    expect(host.textContent).not.toContain('is typing');
    expect(host.querySelector('.ck-potato-bubble--bot')?.textContent).toBe('Fish heard: hello');
    expect(host.querySelector('.ck-potato-mood')?.textContent).toBe('Genki · energy 60');
  });

  it('switches the voice and shows the note row', async () => {
    const transport = fakePotatoTransport({ bots: [bot()] });
    await mount(transport);
    await act(async () => buttonByText(host, 'Potato')!.click());
    await act(async () => {});
    expect(host.querySelector('.ck-potato-note')?.textContent).toBe('Now talking as Potato.');
    expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe('Potato');
  });

  it('makes a bot of a companion with one click and opens it', async () => {
    const transport = fakePotatoTransport({ bots: [] });
    const onOpenBot = vi.fn();
    await mount(transport, [companion()], onOpenBot);
    expect(host.textContent).toContain('No bots yet. Make one to start chatting.');
    await act(async () => buttonByText(host, 'Make Fish a bot')!.click());
    await act(async () => {});
    expect(transport.calls.find((c) => c.method === 'addBot')?.args).toEqual(['fish']);
    expect(onOpenBot).toHaveBeenCalledWith('fish');
    expect(host.querySelector('h1')?.textContent).toBe('Fish');
  });

  it('New bot opens the companion sheet', async () => {
    const transport = fakePotatoTransport({ bots: [bot()] });
    await mount(transport, [companion()]);
    await act(async () => buttonByText(host, 'New bot')!.click());
    await act(async () => {});
    expect(document.body.textContent).toContain('Make your companion');
  });
});
