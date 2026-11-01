// @vitest-environment jsdom
// The three-region page: the bot list, the thread and the bot panel with its Computer / Routines / Advanced tabs. The
// panel opens from the thread's header or from Ctrl+Shift+B; sizes and open/closed are remembered; the Computer tab keeps
// everything the old inline panel did; and a bot that needs you is never missed, even with the panel closed.
import { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakeCompanionTransport, type CompanionView } from '../../../lib/companion';
import { fakePotatoTransport, type FakePotatoTransport, type PotatoState } from '../../../lib/potato';
import { fakeBotSettingsTransport } from '../../../lib/potato-botsettings';
import { LAYOUT_KEYS } from '../../../lib/potato-layout';
import { fakeScreenTransport, type FakeScreenTransport } from '../../../lib/potatoScreen';
import { usePotatoState } from '../../../hooks/usePotato';
import { PotatoPage } from './PotatoPage';
import { bot, buttonByText } from './test-helpers';

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

const companion = (id = 'fish', name = 'Fish'): CompanionView =>
  ({ id, name, avatar: 'bit', persona: 'cheerful', agentType: 'claude', agentLabel: 'Claude', folderName: id, state: 'idle', stateLine: 'Here.', paneId: `companion-${id}`, needs: null, pendingMemory: 0, nextCheckin: null, restarts: 0, autoStart: true }) as CompanionView;

function Harness({ transport, screen, companions }: { transport: FakePotatoTransport; screen?: FakeScreenTransport; companions: CompanionView[] }) {
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
      onOpenBot={setOpenId}
      onChanged={potato.refresh}
      onCompanionsChanged={() => {}}
      onNotify={() => {}}
      onOpenSettings={() => {}}
      screenTransport={screen}
      botSettings={fakeBotSettingsTransport()}
    />
  );
}

async function mount(opts: { screen?: FakeScreenTransport; bots?: ReturnType<typeof bot>[] } = {}) {
  const transport = fakePotatoTransport({ bots: opts.bots ?? [bot()] });
  await act(async () => root.render(<Harness transport={transport} screen={opts.screen} companions={[companion(), companion('bee', 'Bee')]} />));
  await act(async () => {});
  return transport;
}

const page = () => host.querySelector<HTMLElement>('.ck-potato-page')!;
const panel = () => host.querySelector<HTMLElement>('.ck-potato-panel');
const toggle = () => host.querySelector<HTMLButtonElement>('.ck-potato-panel-toggle')!;
const click = async (el: Element | null | undefined) => {
  expect(el).toBeTruthy();
  await act(async () => (el as HTMLElement).click());
  await act(async () => {});
};
const keyDown = (init: KeyboardEventInit) => act(() => void window.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })));
const separator = (label: string) => host.querySelector<HTMLElement>(`[role="separator"][aria-label="${label}"]`)!;
const press = (el: HTMLElement, k: string) => act(() => void el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true })));
const tab = (name: string) => [...host.querySelectorAll('[role="tab"]')].find((t) => t.textContent === name);
const tabs = () => [...host.querySelectorAll('.ck-potato-panel [role="tab"]')].map((t) => t.textContent);
const visiblePanel = () => host.querySelector('.ck-potato-panel [role="tabpanel"]:not([hidden])')?.getAttribute('aria-label');
const state = () => host.querySelector('.ck-potato-computer-state')?.textContent;
const row = (name: string) => [...host.querySelectorAll('.ck-potato-conv')].find((b) => b.textContent?.includes(name));

describe('the three regions', () => {
  it('starts with the list and the thread, and no panel', async () => {
    await mount({ screen: fakeScreenTransport() });
    expect(host.querySelector('.ck-potato-list')).not.toBeNull();
    expect(host.querySelector('.ck-potato-thread')).not.toBeNull();
    expect(panel()).toBeNull();
    expect(page().style.getPropertyValue('--potato-list-w')).toBe('240px');
  });

  it('opens the panel from the thread header and closes it from the panel or the header', async () => {
    await mount({ screen: fakeScreenTransport() });
    await click(toggle());
    expect(panel()).not.toBeNull();
    expect(panel()!.querySelector('h2')?.textContent).toBe('Fish');
    expect(toggle().getAttribute('aria-pressed')).toBe('true');
    await click(host.querySelector('.ck-potato-panel button[aria-label="Close bot panel"]'));
    expect(panel()).toBeNull();
    await click(toggle());
    await click(toggle());
    expect(panel()).toBeNull();
  });

  it('has the tabs Computer, Routines and Advanced: Routines says "Coming soon", Advanced has its sections', async () => {
    await mount({ screen: fakeScreenTransport() });
    await click(toggle());
    expect(tabs()).toEqual(['Computer', 'Routines', 'Advanced']);
    expect(visiblePanel()).toBe('Computer');
    await click(tab('Routines'));
    expect(visiblePanel()).toBe('Routines');
    expect(host.querySelector('[role="tabpanel"][aria-label="Routines"]')!.textContent).toContain('No routines yet.');
    await click(tab('Advanced'));
    expect(visiblePanel()).toBe('Advanced');
    const advanced = host.querySelector('[role="tabpanel"][aria-label="Advanced"]')!;
    expect(advanced.textContent).toContain('Model');
    expect([...advanced.querySelectorAll('.ck-potato-psec-title')].map((t) => t.textContent)).toEqual(['Notifications', 'Open work', 'Memory']);
  });

  it("remembers each bot's tab while the page is open", async () => {
    await mount({ screen: fakeScreenTransport(), bots: [bot(), bot({ id: 'bee', name: 'Bee' })] });
    await click(toggle());
    await click(tab('Advanced'));
    await click(row('Bee'));
    expect(panel()!.querySelector('h2')?.textContent).toBe('Bee');
    expect(visiblePanel()).toBe('Computer');
    await click(row('Fish'));
    expect(visiblePanel()).toBe('Advanced');
  });
});

describe('what is remembered', () => {
  it('keeps the panel open and its width, and the list width, for the next time', async () => {
    await mount({ screen: fakeScreenTransport() });
    await click(toggle());
    press(separator('Resize the bot panel'), 'ArrowLeft'); // grows the panel: 384 + 32
    press(separator('Resize the bot list'), 'ArrowRight'); // 240 + 32
    expect(window.localStorage.getItem(LAYOUT_KEYS.panelOpen)).toBe('1');
    expect(window.localStorage.getItem(LAYOUT_KEYS.panelWidth)).toBe('416');
    expect(window.localStorage.getItem(LAYOUT_KEYS.listWidth)).toBe('272');

    act(() => root.unmount());
    root = createRoot(host);
    await mount({ screen: fakeScreenTransport() });
    expect(panel()).not.toBeNull();
    expect(page().style.getPropertyValue('--potato-panel-w')).toBe('416px');
    expect(page().style.getPropertyValue('--potato-list-w')).toBe('272px');
  });

  it('opens closed again when it was closed', async () => {
    await mount({ screen: fakeScreenTransport() });
    await click(toggle());
    await click(toggle());
    act(() => root.unmount());
    root = createRoot(host);
    await mount({ screen: fakeScreenTransport() });
    expect(panel()).toBeNull();
  });

  it('keeps the strip for the next time', async () => {
    await mount({ screen: fakeScreenTransport() });
    keyDown({ key: 'b', ctrlKey: true });
    act(() => root.unmount());
    root = createRoot(host);
    await mount({ screen: fakeScreenTransport() });
    expect(host.querySelector('.ck-potato-list.is-mini')).not.toBeNull();
  });
});

describe('resizing', () => {
  it('moves the panel with the arrow keys, Home and End, inside 320 to 1000 px', async () => {
    await mount({ screen: fakeScreenTransport() });
    await click(toggle());
    const edge = separator('Resize the bot panel');
    expect(edge.getAttribute('aria-valuenow')).toBe('384');
    press(edge, 'Home');
    expect(edge.getAttribute('aria-valuenow')).toBe('320');
    press(edge, 'ArrowRight'); // right shrinks the panel, but not below its minimum
    expect(edge.getAttribute('aria-valuenow')).toBe('320');
    press(edge, 'End');
    expect(edge.getAttribute('aria-valuenow')).toBe('1000');
  });

  it('moves the list inside 240 to 480 px, and a double-click goes back to 240', async () => {
    await mount({ screen: fakeScreenTransport() });
    const edge = separator('Resize the bot list');
    press(edge, 'End');
    expect(page().style.getPropertyValue('--potato-list-w')).toBe('480px');
    await act(async () => void edge.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })));
    expect(page().style.getPropertyValue('--potato-list-w')).toBe('240px');
  });

  it('follows the pointer when dragged, and snaps to the strip below 160 px', async () => {
    await mount({ screen: fakeScreenTransport() });
    const edge = separator('Resize the bot list');
    const pointer = (type: string, x: number) => act(() => void edge.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, button: 0 })));
    pointer('pointerdown', 240);
    pointer('pointermove', 330);
    expect(page().style.getPropertyValue('--potato-list-w')).toBe('330px');
    pointer('pointermove', 120);
    expect(page().classList.contains('is-mini')).toBe(true);
    pointer('pointerup', 120);
    // a move after the button is up does nothing
    pointer('pointermove', 400);
    expect(page().classList.contains('is-mini')).toBe(true);
  });
});

describe('the keys', () => {
  it('Ctrl+B makes the list a strip and brings it back; Ctrl+Shift+B opens and closes the panel', async () => {
    await mount({ screen: fakeScreenTransport() });
    expect(host.querySelector('.ck-potato-list.is-mini')).toBeNull();
    keyDown({ key: 'b', ctrlKey: true });
    expect(host.querySelector('.ck-potato-list.is-mini')).not.toBeNull();
    expect(window.localStorage.getItem(LAYOUT_KEYS.listMini)).toBe('1');
    keyDown({ key: 'b', metaKey: true });
    expect(host.querySelector('.ck-potato-list.is-mini')).toBeNull();

    expect(panel()).toBeNull();
    keyDown({ key: 'B', ctrlKey: true, shiftKey: true });
    expect(panel()).not.toBeNull();
    keyDown({ key: 'B', metaKey: true, shiftKey: true });
    expect(panel()).toBeNull();
  });

  it("an arrow key on the strip's edge opens the list again", async () => {
    await mount({ screen: fakeScreenTransport() });
    keyDown({ key: 'b', ctrlKey: true });
    press(separator('Resize the bot list'), 'ArrowRight');
    expect(host.querySelector('.ck-potato-list.is-mini')).toBeNull();
    expect(page().style.getPropertyValue('--potato-list-w')).toBe('240px');
  });
});

describe('the Computer tab', () => {
  it('has the computer: a live preview, who has it, Take control (the full window) and I am done', async () => {
    const screen = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await mount({ screen });
    expect(host.querySelector('.ck-potato-computer')).toBeNull();
    await click(toggle());
    expect(host.querySelector('.ck-potato-panel .ck-potato-computer')).not.toBeNull();
    expect(host.querySelector('.ck-potato-panel iframe')).not.toBeNull();
    expect(host.querySelector('.ck-potato-computer-label')?.textContent).toBe('Fish’s screen');
    expect(state()).toBe('Fish is using it');
    await click(buttonByText(host, 'Take control'));
    expect(state()).toBe('You have control');
    const layer = document.querySelector('.ck-potato-overlay');
    expect(layer).not.toBeNull();
    await click([...layer!.querySelectorAll('button')].find((b) => b.textContent === "I'm done"));
    expect(document.querySelector('.ck-potato-overlay')).toBeNull();
    expect(state()).toBe('Not in use');
    expect(screen.calls.map((c) => c.method)).toEqual(expect.arrayContaining(['takeOver', 'handBack']));
  });

  it('stays mounted on the other tabs, so the screen does not reload', async () => {
    await mount({ screen: fakeScreenTransport({ phase: 'bot', bot: 'fish' }) });
    await click(toggle());
    const frame = host.querySelector('iframe');
    expect(frame).not.toBeNull();
    await click(tab('Routines'));
    expect(host.querySelector('iframe')).toBe(frame);
    await click(tab('Computer'));
    expect(host.querySelector('iframe')).toBe(frame);
  });

  it('says why there is no computer when Sandbox is off, or there is no screen transport', async () => {
    await mount({ screen: fakeScreenTransport({ enabled: false }) });
    await click(toggle());
    expect(host.querySelector('.ck-potato-computer')).toBeNull();
    expect(host.querySelector('.ck-potato-panel-empty')?.textContent).toBe('Fish has no computer here. Turn on Sandbox in Settings > General > Labs.');
    act(() => root.unmount());
    root = createRoot(host);
    await mount();
    expect(host.querySelector('.ck-potato-panel-empty')?.textContent).toContain('no computer here');
  });

  it('says so, and asks nothing of the screen, for a bot that talks through the agents', async () => {
    const screen = fakeScreenTransport();
    await mount({ screen, bots: [bot({ brainState: 'agents' })] });
    await click(toggle());
    expect(host.querySelector('.ck-potato-panel-empty')?.textContent).toBe('A bot that talks through your agents has no computer of its own.');
    expect(screen.calls).toHaveLength(0);
  });
});

describe('Needs you', () => {
  const asking = () => fakeScreenTransport({ phase: 'needs_you', bot: 'fish', cardId: 'needs-1', reason: 'The site wants a code' });
  const banner = () => host.querySelector('.ck-potato-banner');
  const openComputer = () => [...banner()!.querySelectorAll('button')].find((b) => b.textContent === 'Open computer');

  it('with the panel closed, the thread has a banner; Open computer opens the panel on the Computer tab', async () => {
    await mount({ screen: asking() });
    expect(panel()).toBeNull();
    expect(banner()).not.toBeNull();
    expect(banner()!.textContent).toContain('Fish needs you on the computer.');
    await click(openComputer());
    expect(panel()).not.toBeNull();
    expect(visiblePanel()).toBe('Computer');
    expect(banner()).toBeNull();
    expect(state()).toBe('Needs you');
    expect(panel()!.textContent).toContain('The site wants a code');
    expect(buttonByText(host, 'Take control')).toBeTruthy();
    expect(buttonByText(host, 'Skip')).toBeTruthy();
  });

  it('on another tab the banner is still there and brings the person back to the Computer tab', async () => {
    await mount({ screen: asking() });
    await click(toggle());
    await click(tab('Advanced'));
    expect(banner()).not.toBeNull();
    await click(openComputer());
    expect(visiblePanel()).toBe('Computer');
    expect(banner()).toBeNull();
  });

  it('shows up by itself when a bot asks while the panel is closed, and goes when the card is skipped', async () => {
    const screen = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await mount({ screen });
    expect(banner()).toBeNull();
    await act(async () => screen.set({ phase: 'needs_you', bot: 'fish', cardId: 'needs-2', reason: 'Look at this' }));
    expect(banner()).not.toBeNull();
    await click(openComputer());
    await click(buttonByText(host, 'Skip'));
    expect(screen.calls.find((c) => c.method === 'skip')!.args).toEqual(['needs-2']);
    expect(banner()).toBeNull();
    expect(state()).toBe('Fish is using it');
  });
});
