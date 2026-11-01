// @vitest-environment jsdom
// The Computer panel (top of the Computer tab): hidden while the Labs switches are off, the preview card and what it says
// when there is nothing to see, the three header phrases, Take control and the full-window computer (I'm done, Esc and Close
// with one question, Skip), the status from the viewer (believed only from the iframe's own window and origin), Retry, the
// lease kept alive, and the password staying in the iframe address.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeScreenTransport, FAKE_ORIGIN, type FakeScreenTransport, type ScreenTransport } from '../../../lib/potatoScreen';
import type { SandboxState } from '../../../lib/potato-sandbox';
import { ComputerPanel, KEEP_EVERY_MS, SILENT_VIEWER_MS } from './ComputerPanel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
  vi.useRealTimers();
});

const BOT = { id: 'fish', name: 'Fish' };

interface ShowOptions {
  onNotify?: (t: string) => void;
  sandboxState?: SandboxState | null;
  onWake?: () => void;
}
async function show(transport: ScreenTransport, options: ShowOptions = {}) {
  await act(async () => root.render(<ComputerPanel bot={BOT} transport={transport} {...options} />));
  await act(async () => {});
}

const overlay = () => document.querySelector<HTMLElement>('.ck-potato-overlay');
/** A button by its text in the panel, or in the full window while it is open. */
const nameOf = (b: HTMLButtonElement) => b.getAttribute('aria-label') ?? b.textContent?.trim();
const byName = (within: ParentNode, label: string) => [...within.querySelectorAll<HTMLButtonElement>('button')].find((b) => nameOf(b) === label);
const find = (label: string) => byName(overlay() ?? host, label) ?? byName(host, label);
const click = async (label: string) => {
  const button = find(label);
  expect(button, `a button named ${label}`).toBeTruthy();
  await act(async () => button!.click());
  await act(async () => {});
};
const clickEl = async (el: Element | null | undefined) => {
  expect(el).toBeTruthy();
  await act(async () => (el as HTMLElement).click());
  await act(async () => {});
};

const header = () => host.querySelector('.ck-potato-computer-state')?.textContent;
const frame = () => document.querySelector('iframe');
const note = () => (overlay() ?? host).querySelector('.ck-potato-computer-note')?.textContent ?? '';
const buttons = (within: ParentNode = host) => [...within.querySelectorAll<HTMLButtonElement>('button')].map(nameOf);
const pressKey = (key: string) => act(async () => void window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })));

/** The viewer's message, as the browser would deliver it. */
async function viewerSays(data: unknown, over: { origin?: string; source?: Window | null } = {}) {
  const iframe = frame()!;
  await act(async () => {
    window.dispatchEvent(
      new MessageEvent('message', { data, origin: over.origin ?? FAKE_ORIGIN, source: over.source === undefined ? iframe.contentWindow : over.source }),
    );
  });
}
const status = (s: string, reason?: string, title?: string) => ({ type: 'pm-sbx-screen-status', status: s, ...(reason ? { reason } : {}), ...(title ? { title } : {}) });

describe('the Labs switches', () => {
  it('draws nothing while the screen is off, and asks for no link', async () => {
    const t = fakeScreenTransport({ enabled: false });
    await show(t);
    expect(host.innerHTML).toBe('');
    expect(t.calls.map((c) => c.method)).toEqual(['state']);
  });

  it('draws nothing when the app cannot say', async () => {
    const t = fakeScreenTransport();
    t.failNext('state', 'NOT_READY: not yet');
    await show(t);
    expect(host.innerHTML).toBe('');
  });

  it('goes away when the switch is turned off, and comes back', async () => {
    const t = fakeScreenTransport();
    await show(t);
    expect(host.querySelector('.ck-potato-computer')).toBeTruthy();
    await act(async () => t.set({ enabled: false }));
    expect(host.innerHTML).toBe('');
    await act(async () => t.set({ enabled: true }));
    expect(host.querySelector('.ck-potato-computer')).toBeTruthy();
  });
});

describe('the preview card', () => {
  it('is live at once, in the view mode, and says "<Bot>’s screen"', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    expect(t.calls.find((c) => c.method === 'link')!.args).toEqual(['fish', 'view']);
    expect(frame()!.getAttribute('src')).toMatch(/^http:\/\/127\.0\.0\.1:41999\/bridge\/potato\/sandbox\/screen\/FAKE1\/embed\.html#vnc=/);
    expect(frame()!.getAttribute('src')).not.toContain('control');
    expect(frame()!.getAttribute('sandbox')).toBe('allow-scripts allow-same-origin');
    expect(host.querySelector('.ck-potato-computer-label')?.textContent).toBe('Fish’s screen');
    expect(buttons()).toEqual(['Open in full window', 'Take control']);
  });

  it('shows the title of the window the bot is on, when the viewer says it', async () => {
    await show(fakeScreenTransport({ phase: 'bot', bot: 'fish' }));
    expect(host.querySelector('.ck-potato-computer-window')).toBeNull();
    await viewerSays(status('connected', undefined, 'Inbox - Chromium'));
    expect(host.querySelector('.ck-potato-computer-window')?.textContent).toBe('Inbox - Chromium');
  });

  it('opens the full window in the view mode from its "Open" button, and lets go of the frame', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await clickEl(host.querySelector('.ck-potato-computer-open'));
    expect(overlay()).toBeTruthy();
    expect(t.calls.some((c) => c.method === 'takeOver')).toBe(false);
    expect(t.calls.filter((c) => c.method === 'link').at(-1)!.args).toEqual(['fish', 'view']);
    // one viewer at a time: the card says where the screen is
    expect(host.querySelector('iframe')).toBeNull();
    expect(host.querySelector('.ck-potato-computer-view')?.textContent).toContain('Open in full window');
    // looking only: closing needs no question
    await click('Close');
    expect(overlay()).toBeNull();
    expect(host.querySelector('iframe')).toBeTruthy();
    expect(t.calls.some((c) => c.method === 'handBack')).toBe(false);
  });

  it('says Asleep with a Wake button for a stopped computer, and asks for no link', async () => {
    const t = fakeScreenTransport({ phase: 'free' });
    const wake = vi.fn();
    await show(t, { sandboxState: 'stopped', onWake: wake });
    expect(host.querySelector('.ck-potato-computer-view')?.textContent).toContain('Asleep');
    expect(host.querySelector('iframe')).toBeNull();
    expect(t.calls.some((c) => c.method === 'link')).toBe(false);
    expect(find('Take control')!.disabled).toBe(true);
    await click('Wake');
    expect(wake).toHaveBeenCalled();
  });

  it('says it is waking while the computer starts, and when it could not', async () => {
    await show(fakeScreenTransport(), { sandboxState: 'starting', onWake: () => {} });
    expect(host.querySelector('.ck-potato-computer-view')?.textContent).toContain('Waking up');
    expect(find('Wake')).toBeUndefined();
    await show(fakeScreenTransport(), { sandboxState: 'failed', onWake: () => {} });
    expect(host.querySelector('.ck-potato-computer-view')?.textContent).toContain('Could not start');
    expect(find('Start again')).toBeTruthy();
  });

  it('goes live when the computer wakes', async () => {
    const t = fakeScreenTransport();
    await show(t, { sandboxState: 'stopped', onWake: () => {} });
    expect(frame()).toBeNull();
    await show(t, { sandboxState: 'running', onWake: () => {} });
    expect(frame()).toBeTruthy();
    expect(find('Take control')!.disabled).toBe(false);
  });
});

describe('the header', () => {
  it('says who has the screen', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    expect(header()).toBe('Fish is using it');
    await act(async () => t.set({ phase: 'you', bot: null }));
    expect(header()).toBe('You have control');
    await act(async () => t.set({ phase: 'needs_you', bot: 'fish', cardId: 'needs-1', reason: 'Type the code from your phone' }));
    expect(header()).toBe('Needs you');
    expect(host.textContent).toContain('Type the code from your phone');
    await act(async () => t.set({ phase: 'free', bot: null, cardId: null, reason: null }));
    expect(header()).toBe('Not in use');
  });

  it('does not call another bot by this bot name', async () => {
    await show(fakeScreenTransport({ phase: 'bot', bot: 'someone-else' }));
    expect(header()).toBe('Another bot is using it');
  });
});

describe('Take control and the full window', () => {
  it('Take control takes the screen and opens the full window in the control mode', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Take control');
    expect(t.calls.map((c) => c.method)).toContain('takeOver');
    expect(header()).toBe('You have control');
    const layer = overlay()!;
    expect(layer).toBeTruthy();
    expect(layer.getAttribute('role')).toBe('dialog');
    expect(layer.querySelector('.ck-potato-overlay-title')?.textContent).toBe('Fish’s computer');
    expect(layer.querySelector('.ck-potato-overlay-state')?.textContent).toBe('You have control');
    expect(t.calls.filter((c) => c.method === 'link').at(-1)!.args).toEqual(['fish', 'control']);
    expect(frame()!.getAttribute('src')).toContain('?control=1');
    expect(buttons(layer)).toEqual(["I'm done", 'Close']);
    // the keyboard starts on the layer, so Esc works at once
    expect(document.activeElement).toBe(layer);
  });

  it("I'm done hands the screen back, closes the window and returns the keyboard to Take control", async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Take control');
    await click("I'm done");
    expect(t.calls.map((c) => c.method)).toContain('handBack');
    expect(overlay()).toBeNull();
    expect(header()).toBe('Not in use');
    expect(document.activeElement).toBe(find('Take control'));
    expect(t.calls.filter((c) => c.method === 'link').at(-1)!.args).toEqual(['fish', 'view']);
    expect(frame()!.getAttribute('src')).not.toContain('control');
  });

  it('Esc asks once; "Keep control" goes on, "Hand back" hands it back', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Take control');
    await pressKey('Escape');
    const question = overlay()!.querySelector('[role="alertdialog"]');
    expect(question?.textContent).toContain('Hand the computer back to Fish?');
    expect(t.calls.some((c) => c.method === 'handBack')).toBe(false);
    await click('Keep control');
    expect(overlay()!.querySelector('[role="alertdialog"]')).toBeNull();
    expect(overlay()).toBeTruthy();
    expect(header()).toBe('You have control');
    // a second Esc while the question is up takes the question away
    await pressKey('Escape');
    expect(overlay()!.querySelector('[role="alertdialog"]')).toBeTruthy();
    await pressKey('Escape');
    expect(overlay()!.querySelector('[role="alertdialog"]')).toBeNull();
    await pressKey('Escape');
    await click('Hand back');
    expect(t.calls.filter((c) => c.method === 'handBack')).toHaveLength(1);
    expect(overlay()).toBeNull();
    expect(header()).toBe('Not in use');
  });

  it('Close asks the same question while the person has the screen', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Take control');
    await click('Close');
    expect(overlay()!.querySelector('[role="alertdialog"]')).toBeTruthy();
    expect(overlay()).toBeTruthy();
    await click('Hand back');
    expect(overlay()).toBeNull();
    expect(t.calls.map((c) => c.method)).toContain('handBack');
  });

  it('a bot that asks: Skip and Take control are in the full window, Skip turns the card down', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await clickEl(host.querySelector('.ck-potato-computer-open'));
    await act(async () => t.set({ phase: 'needs_you', bot: 'fish', cardId: 'needs-9', reason: 'Solve the check' }));
    const layer = overlay()!;
    expect(layer.querySelector('.ck-potato-overlay-state')?.textContent).toBe('Needs you');
    expect(layer.textContent).toContain('Solve the check');
    expect(buttons(layer)).toEqual(['Skip', 'Take control', 'Close']);
    await click('Skip');
    expect(t.calls.find((c) => c.method === 'skip')!.args).toEqual(['needs-9']);
    expect(overlay()).toBeTruthy();
    expect(buttons(overlay()!)).not.toContain('Skip');
    expect(overlay()!.querySelector('.ck-potato-overlay-state')?.textContent).toBe('Fish is using it');
  });

  it('Take control inside the full window takes the screen and switches to the control view', async () => {
    const t = fakeScreenTransport({ phase: 'needs_you', bot: 'fish', cardId: 'needs-7', reason: 'x' });
    await show(t);
    await clickEl(host.querySelector('.ck-potato-computer-open'));
    expect(frame()!.getAttribute('src')).not.toContain('control');
    await click('Take control');
    expect(t.calls.map((c) => c.method)).toContain('takeOver');
    expect(frame()!.getAttribute('src')).toContain('?control=1');
    expect(buttons(overlay()!)).toContain("I'm done");
  });

  it('the window goes away when the person loses the screen some other way', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Take control');
    expect(overlay()).toBeTruthy();
    await act(async () => t.set({ phase: 'free', bot: null }));
    expect(overlay()).toBeNull();
  });
});

describe('the buttons', () => {
  it('Take control while the bot uses it, then I’m done from the card too', async () => {
    const t = fakeScreenTransport({ phase: 'you' });
    await show(t);
    expect(buttons()).toEqual(['Open in full window', 'Open', "I'm done"]);
    await click("I'm done");
    expect(header()).toBe('Not in use');
    expect(buttons()).toEqual(['Open in full window', 'Take control']);
  });

  it('a "Needs you" card has Skip and Take control, and Skip turns it down', async () => {
    const t = fakeScreenTransport({ phase: 'needs_you', bot: 'fish', cardId: 'needs-7', reason: 'Solve the check' });
    await show(t);
    expect(buttons()).toEqual(['Open in full window', 'Skip', 'Take control']);
    await click('Skip');
    expect(t.calls.find((c) => c.method === 'skip')!.args).toEqual(['needs-7']);
    expect(header()).toBe('Fish is using it');
    expect(buttons()).not.toContain('Skip');
  });

  it('a button that fails says one plain sentence, reads the state again and opens no window', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    const told = vi.fn();
    await show(t, { onNotify: told });
    t.failNext('takeOver', 'SCREEN_OFF: The Computer is turned off.');
    await click('Take control');
    expect(told).toHaveBeenCalledWith('The Computer is turned off.');
    expect(t.calls.filter((c) => c.method === 'state').length).toBeGreaterThanOrEqual(2);
    expect(header()).toBe('Fish is using it');
    expect(overlay()).toBeNull();
  });

  it('a hand back that fails keeps the window open', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    const told = vi.fn();
    await show(t, { onNotify: told });
    await click('Take control');
    t.failNext('handBack', 'SCREEN_NOT_YOURS: You do not have it.');
    await click("I'm done");
    expect(told).toHaveBeenCalledWith('You do not have it.');
    expect(overlay()).toBeTruthy();
  });
});

describe('the screen', () => {
  it('the password is in the iframe address and nowhere else on the page', async () => {
    await show(fakeScreenTransport({ phase: 'bot', bot: 'fish' }));
    expect(host.textContent).not.toContain('TESTPASS');
    expect(host.innerHTML.split('TESTPASS')).toHaveLength(2); // exactly once: the src attribute
    expect(frame()!.getAttribute('src')).toContain('#vnc=TESTPASS');
    expect(host.querySelector('[title*="TESTPASS"], [aria-label*="TESTPASS"], [data-src]')).toBeNull();
    // and in the full window the same
    await click('Take control');
    expect(overlay()!.textContent).not.toContain('TESTPASS');
    expect(overlay()!.innerHTML.split('TESTPASS')).toHaveLength(2);
  });

  it('shows "Connecting…" until the viewer says it is connected', async () => {
    await show(fakeScreenTransport({ phase: 'bot', bot: 'fish' }));
    expect(note()).toContain('Connecting…');
    await viewerSays(status('connecting'));
    expect(note()).toContain('Connecting…');
    await viewerSays(status('connected'));
    expect(note()).toBe('');
    expect(buttons()).not.toContain('Retry');
  });

  it('a failed viewer says why and offers Retry, which makes a new link', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await viewerSays(status('failed', 'timeout'));
    expect(note()).toContain('It took too long to connect.');
    expect(buttons()).toContain('Retry');
    await click('Retry');
    expect(t.calls.filter((c) => c.method === 'link')).toHaveLength(2);
    expect(frame()!.getAttribute('src')).toContain('/FAKE2/');
    expect(note()).toContain('Connecting…');
    await viewerSays(status('connected'));
    expect(note()).toBe('');
  });

  it('a closed connection offers Retry too', async () => {
    await show(fakeScreenTransport({ phase: 'bot', bot: 'fish' }));
    await viewerSays(status('connected'));
    await viewerSays(status('disconnected', 'closed'));
    expect(note()).toContain('The connection closed.');
    expect(buttons()).toContain('Retry');
  });

  it('a link that cannot be made says the plain sentence and offers Retry', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    t.failNext('link', 'SCREEN_NOT_RUNNING: The sandbox is not running yet.');
    await show(t);
    expect(note()).toContain('The sandbox is not running yet.');
    expect(note()).not.toContain('SCREEN_NOT_RUNNING');
    expect(frame()).toBeNull();
    await click('Retry');
    expect(frame()).toBeTruthy();
  });

  it('a viewer that never reports is called failed after a while', async () => {
    vi.useFakeTimers();
    await show(fakeScreenTransport({ phase: 'bot', bot: 'fish' }));
    expect(note()).toContain('Connecting…');
    await act(async () => void vi.advanceTimersByTime(SILENT_VIEWER_MS + 10));
    expect(note()).toContain('It took too long to connect.');
    expect(buttons()).toContain('Retry');
  });

  it('the full window has the same Retry', async () => {
    await show(fakeScreenTransport({ phase: 'bot', bot: 'fish' }));
    await click('Take control');
    await viewerSays(status('failed', 'security'));
    expect(note()).toContain('The screen did not accept the link.');
    expect(buttons(overlay()!)).toContain('Retry');
  });
});

describe('who is believed', () => {
  async function open() {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    return t;
  }

  it('a message from another origin is ignored', async () => {
    await open();
    await viewerSays(status('connected'), { origin: 'https://evil.example' });
    expect(note()).toContain('Connecting…');
    await viewerSays(status('connected'), { origin: 'http://127.0.0.1:41998' });
    expect(note()).toContain('Connecting…');
    await viewerSays(status('connected'), { origin: 'http://tauri.localhost' });
    expect(note()).toContain('Connecting…');
  });

  it('a message from another window is ignored, even with the right origin', async () => {
    await open();
    await viewerSays(status('connected'), { source: window });
    expect(note()).toContain('Connecting…');
    await viewerSays(status('connected'), { source: null });
    expect(note()).toContain('Connecting…');
  });

  it('a message that is not a status is ignored', async () => {
    await open();
    for (const data of ['connected', null, {}, { type: 'other', status: 'connected' }, { type: 'pm-sbx-screen-status', status: 'fine' }]) {
      await viewerSays(data);
    }
    expect(note()).toContain('Connecting…');
  });

  it('the right window and origin is believed', async () => {
    await open();
    await viewerSays(status('connected'));
    expect(note()).toBe('');
  });

  it('a title from a window or origin that is not believed is not shown', async () => {
    await open();
    await viewerSays(status('connected', undefined, 'Sneaky'), { origin: 'https://evil.example' });
    expect(host.querySelector('.ck-potato-computer-window')).toBeNull();
  });

  it('once the panel is gone, nothing is listened to', async () => {
    const spy = vi.spyOn(window, 'removeEventListener');
    await open();
    act(() => root.unmount());
    expect(spy.mock.calls.some(([type]) => type === 'message')).toBe(true);
    spy.mockRestore();
    root = createRoot(host);
  });
});

describe('keeping the lease', () => {
  it('asks the app to keep it while the person works in the screen of the full window', async () => {
    vi.useFakeTimers();
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Take control');
    expect(frame()!.getAttribute('src')).toContain('?control=1');
    // the person is elsewhere on the page: nothing is kept
    await act(async () => void vi.advanceTimersByTime(KEEP_EVERY_MS + 10));
    expect(t.calls.some((c) => c.method === 'keep')).toBe(false);
    // the iframe has the keyboard
    await act(async () => frame()!.focus());
    expect(document.activeElement).toBe(frame());
    await act(async () => void vi.advanceTimersByTime(KEEP_EVERY_MS));
    expect(t.calls.filter((c) => c.method === 'keep').length).toBeGreaterThanOrEqual(1);
  });

  it('keeps nothing in the view mode', async () => {
    vi.useFakeTimers();
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Open in full window');
    await act(async () => frame()!.focus());
    await act(async () => void vi.advanceTimersByTime(KEEP_EVERY_MS * 3));
    expect(t.calls.some((c) => c.method === 'keep')).toBe(false);
  });

  it('stops keeping it once the screen is handed back', async () => {
    vi.useFakeTimers();
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Take control');
    await click("I'm done");
    await act(async () => frame()?.focus());
    await act(async () => void vi.advanceTimersByTime(KEEP_EVERY_MS * 3));
    expect(t.calls.some((c) => c.method === 'keep')).toBe(false);
  });
});

describe('a lasting link', () => {
  it('is renewed before the hour is up', async () => {
    vi.useFakeTimers();
    const t: FakeScreenTransport = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    expect(t.calls.filter((c) => c.method === 'link')).toHaveLength(1);
    await act(async () => void vi.advanceTimersByTime(3_600_000 - 30_000));
    expect(t.calls.filter((c) => c.method === 'link').length).toBeGreaterThanOrEqual(2);
  });
});
