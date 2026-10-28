// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { browserTelegramDeps, TelegramOffRow, type TelegramOffDeps } from './TelegramOffRow';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function fakeDeps(over: Partial<TelegramOffDeps> = {}): TelegramOffDeps {
  return {
    status: vi.fn(async () => ({ on: true })),
    turnOff: vi.fn(async () => undefined),
    ...over,
  };
}

describe('TelegramOffRow', () => {
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
    vi.unstubAllGlobals();
  });

  const settle = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });
  const button = () => host.querySelector('button') as HTMLButtonElement | null;

  it('renders nothing when the PC says Telegram is off (not paired or not turned on)', async () => {
    const deps = fakeDeps({ status: vi.fn(async () => ({ on: false })) });
    await act(async () => root.render(<TelegramOffRow baseUrl="u" bridgeReady deps={deps} />));
    await settle();
    expect(deps.status).toHaveBeenCalledTimes(1);
    expect(host.innerHTML).toBe('');
  });

  it('renders nothing when the status read fails', async () => {
    const deps = fakeDeps({ status: vi.fn(async () => { throw new Error('GET /channels/telegram -> 503'); }) });
    await act(async () => root.render(<TelegramOffRow baseUrl="u" bridgeReady deps={deps} />));
    await settle();
    expect(host.innerHTML).toBe('');
  });

  it('asks nothing and renders nothing while the bridge is not ready', async () => {
    const deps = fakeDeps();
    await act(async () => root.render(<TelegramOffRow baseUrl="u" bridgeReady={false} deps={deps} />));
    await settle();
    expect(deps.status).not.toHaveBeenCalled();
    expect(host.innerHTML).toBe('');
  });

  it('shows one Turn Telegram off button when on, and the off line after a tap', async () => {
    const deps = fakeDeps();
    await act(async () => root.render(<TelegramOffRow baseUrl="u" bridgeReady deps={deps} />));
    await settle();
    expect(host.textContent).toContain('Telegram · On');
    expect(host.querySelectorAll('button')).toHaveLength(1);
    expect(button()?.textContent).toBe('Turn Telegram off');
    expect(button()?.disabled).toBe(false);

    await act(async () => button()!.click());
    await settle();
    expect(deps.turnOff).toHaveBeenCalledTimes(1);
    expect(host.querySelectorAll('button')).toHaveLength(0);
    expect(host.textContent).toContain('Off. Turn it on again on your PC.');
    expect(host.textContent).not.toContain('Turn on');
  });

  it('keeps the button and shows an alert when the PC does not answer', async () => {
    const deps = fakeDeps({ turnOff: vi.fn(async () => { throw new Error('POST /channels/telegram/off -> 500'); }) });
    await act(async () => root.render(<TelegramOffRow baseUrl="u" bridgeReady deps={deps} />));
    await settle();
    await act(async () => button()!.click());
    await settle();
    expect(button()).not.toBeNull();
    expect(button()?.disabled).toBe(false);
    const alert = host.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain("Couldn't turn Telegram off. Your PC didn't answer.");
    expect(host.textContent).not.toContain('POST /channels');
  });

  it('disables the button while the bridge is not ready', async () => {
    const deps = fakeDeps();
    await act(async () => root.render(<TelegramOffRow baseUrl="u" bridgeReady deps={deps} />));
    await settle();
    await act(async () => root.render(<TelegramOffRow baseUrl="u" bridgeReady={false} deps={deps} />));
    await settle();
    expect(button()?.disabled).toBe(true);
  });
});

describe('browserTelegramDeps (bridge calls)', () => {
  it('reads the status with GET and switches off with POST on the phone route', async () => {
    const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.endsWith('/channels/telegram/off')) return new Response(JSON.stringify({ ok: true, wasOn: true }), { status: 200 });
      return new Response(JSON.stringify({ on: true, connected: true, botUsername: 'anna_pm_bot' }), { status: 200 });
    });
    vi.stubGlobal('fetch', fetchMock);
    const deps = browserTelegramDeps('https://pm.example.com/bridge');

    await expect(deps.status()).resolves.toEqual({ on: true, connected: true, botUsername: 'anna_pm_bot' });
    await deps.turnOff();

    expect(fetchMock.mock.calls[0][0]).toBe('https://pm.example.com/bridge/channels/telegram');
    expect(fetchMock.mock.calls[0][1]?.method).toBe('GET');
    expect(fetchMock.mock.calls[1][0]).toBe('https://pm.example.com/bridge/channels/telegram/off');
    expect(fetchMock.mock.calls[1][1]?.method).toBe('POST');
    vi.unstubAllGlobals();
  });

  it('turns a non-OK answer into an error without the URL in the message', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 403 })));
    const deps = browserTelegramDeps('https://pm.example.com/bridge');
    await expect(deps.turnOff()).rejects.toThrow('POST /channels/telegram/off -> 403');
    vi.unstubAllGlobals();
  });
});
