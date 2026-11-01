// @vitest-environment jsdom
// The Computer panel: hidden while the Labs switches are off, the three header phrases, the buttons, the status from the
// viewer (believed only from the iframe's own window and origin), Retry, and the password staying in the iframe address.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeScreenTransport, FAKE_ORIGIN, type FakeScreenTransport, type ScreenTransport } from '../../../lib/potatoScreen';
import { ComputerPanel, KEEP_EVERY_MS, SILENT_VIEWER_MS } from './ComputerPanel';
import { buttonByText } from './test-helpers';

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

async function show(transport: ScreenTransport, onNotify?: (t: string) => void) {
  await act(async () => root.render(<ComputerPanel bot={BOT} transport={transport} onNotify={onNotify} />));
  await act(async () => {});
}

const click = async (label: string) => {
  const button = buttonByText(host, label);
  expect(button, `a button named ${label}`).toBeTruthy();
  await act(async () => button!.click());
};

const header = () => host.querySelector('.ck-potato-computer-state')?.textContent;
const frame = () => host.querySelector('iframe');
const note = () => host.querySelector('.ck-potato-computer-note')?.textContent ?? '';
const buttons = () => [...host.querySelectorAll('button')].map((b) => b.textContent);

/** The viewer's message, as the browser would deliver it. */
async function viewerSays(data: unknown, over: { origin?: string; source?: Window | null } = {}) {
  const iframe = frame()!;
  await act(async () => {
    window.dispatchEvent(
      new MessageEvent('message', { data, origin: over.origin ?? FAKE_ORIGIN, source: over.source === undefined ? iframe.contentWindow : over.source }),
    );
  });
}
const status = (s: string, reason?: string) => ({ type: 'pm-sbx-screen-status', status: s, ...(reason ? { reason } : {}) });

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

describe('the buttons', () => {
  it('Take over while the bot uses it, then Hand back', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    expect(buttons()).toEqual(['Take over', 'Show']);
    await click('Take over');
    expect(t.calls.map((c) => c.method)).toContain('takeOver');
    expect(header()).toBe('You have control');
    expect(buttons()).toEqual(['Hand back', 'Hide']);
    // taking over opens the screen, and asks for a control link
    expect(frame()).toBeTruthy();
    expect(t.calls.filter((c) => c.method === 'link').at(-1)!.args).toEqual(['fish', 'control']);
    expect(frame()!.getAttribute('src')).toContain('?control=1');

    await click('Hand back');
    expect(t.calls.map((c) => c.method)).toContain('handBack');
    expect(header()).toBe('Not in use');
    expect(t.calls.filter((c) => c.method === 'link').at(-1)!.args).toEqual(['fish', 'view']);
    expect(frame()!.getAttribute('src')).not.toContain('control');
  });

  it('a "Needs you" card has Take over and Skip, and opens by itself', async () => {
    const t = fakeScreenTransport({ phase: 'needs_you', bot: 'fish', cardId: 'needs-7', reason: 'Solve the check' });
    await show(t);
    expect(buttons()).toEqual(['Take over', 'Skip', 'Hide']);
    expect(frame()).toBeTruthy();
    await click('Skip');
    expect(t.calls.find((c) => c.method === 'skip')!.args).toEqual(['needs-7']);
    expect(header()).toBe('Fish is using it');
    expect(buttons()).not.toContain('Skip');
  });

  it('Take over on a card takes the screen', async () => {
    const t = fakeScreenTransport({ phase: 'needs_you', bot: 'fish', cardId: 'needs-7', reason: 'x' });
    await show(t);
    await click('Take over');
    expect(header()).toBe('You have control');
    expect(host.textContent).not.toContain('Solve');
  });

  it('a button that fails says one plain sentence and reads the state again', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    const told = vi.fn();
    await show(t, told);
    t.failNext('takeOver', 'SCREEN_OFF: The Computer is turned off.');
    await click('Take over');
    expect(told).toHaveBeenCalledWith('The Computer is turned off.');
    expect(t.calls.filter((c) => c.method === 'state').length).toBeGreaterThanOrEqual(2);
    expect(header()).toBe('Fish is using it');
  });
});

describe('the screen', () => {
  it('stays closed until Show, then asks for a view link', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    expect(frame()).toBeNull();
    expect(t.calls.some((c) => c.method === 'link')).toBe(false);
    await click('Show');
    expect(t.calls.find((c) => c.method === 'link')!.args).toEqual(['fish', 'view']);
    expect(frame()!.getAttribute('src')).toMatch(/^http:\/\/127\.0\.0\.1:41999\/bridge\/potato\/sandbox\/screen\/FAKE1\/embed\.html#vnc=/);
    expect(frame()!.getAttribute('sandbox')).toBe('allow-scripts allow-same-origin');
    await click('Hide');
    expect(frame()).toBeNull();
  });

  it('the password is in the iframe address and nowhere else on the page', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Show');
    expect(host.textContent).not.toContain('TESTPASS');
    expect(host.innerHTML.split('TESTPASS')).toHaveLength(2); // exactly once: the src attribute
    expect(frame()!.getAttribute('src')).toContain('#vnc=TESTPASS');
    expect(host.querySelector('[title*="TESTPASS"], [aria-label*="TESTPASS"], [data-src]')).toBeNull();
  });

  it('shows "Connecting…" until the viewer says it is connected', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Show');
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
    await click('Show');
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
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Show');
    await viewerSays(status('connected'));
    await viewerSays(status('disconnected', 'closed'));
    expect(note()).toContain('The connection closed.');
    expect(buttons()).toContain('Retry');
  });

  it('a link that cannot be made says the plain sentence and offers Retry', async () => {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    t.failNext('link', 'SCREEN_NOT_RUNNING: The sandbox is not running yet.');
    await click('Show');
    expect(note()).toContain('The sandbox is not running yet.');
    expect(note()).not.toContain('SCREEN_NOT_RUNNING');
    expect(frame()).toBeNull();
    await click('Retry');
    expect(frame()).toBeTruthy();
  });

  it('a viewer that never reports is called failed after a while', async () => {
    vi.useFakeTimers();
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Show');
    expect(note()).toContain('Connecting…');
    await act(async () => void vi.advanceTimersByTime(SILENT_VIEWER_MS + 10));
    expect(note()).toContain('It took too long to connect.');
    expect(buttons()).toContain('Retry');
  });
});

describe('who is believed', () => {
  async function open() {
    const t = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Show');
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

  it('once closed, nothing is listened to', async () => {
    const spy = vi.spyOn(window, 'removeEventListener');
    await open();
    await click('Hide');
    expect(spy.mock.calls.some(([type]) => type === 'message')).toBe(true);
    spy.mockRestore();
  });
});

describe('keeping the lease', () => {
  it('asks the app to keep it while the person works in the screen', async () => {
    vi.useFakeTimers();
    const t = fakeScreenTransport({ phase: 'you' });
    await show(t);
    await click('Show');
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
    await click('Show');
    await act(async () => frame()!.focus());
    await act(async () => void vi.advanceTimersByTime(KEEP_EVERY_MS * 3));
    expect(t.calls.some((c) => c.method === 'keep')).toBe(false);
  });
});

describe('a lasting link', () => {
  it('is renewed before the hour is up', async () => {
    vi.useFakeTimers();
    const t: FakeScreenTransport = fakeScreenTransport({ phase: 'bot', bot: 'fish' });
    await show(t);
    await click('Show');
    expect(t.calls.filter((c) => c.method === 'link')).toHaveLength(1);
    await act(async () => void vi.advanceTimersByTime(3_600_000 - 30_000));
    expect(t.calls.filter((c) => c.method === 'link').length).toBeGreaterThanOrEqual(2);
  });
});
