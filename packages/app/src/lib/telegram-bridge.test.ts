import { afterEach, describe, expect, it, vi } from 'vitest';

const pairing = vi.hoisted(() => ({ creds: null as null | { deviceToken: string } }));
vi.mock('./mobile-pairing', () => ({ loadMobilePairingCredentials: () => pairing.creds }));

import { getTelegramPhoneStatus, turnTelegramOff } from './telegram-bridge';

const BASE = 'http://127.0.0.1:17321';
const TOKEN = 'device-token-for-tests';

/** The error a rejected call throws; fails the test if the call resolves. */
async function errorOf(p: Promise<unknown>): Promise<Error> {
  try {
    await p;
  } catch (e) {
    return e as Error;
  }
  throw new Error('expected the call to fail');
}

function reply(status: number, body: unknown) {
  return new Response(typeof body === 'string' ? body : JSON.stringify(body), { status });
}

afterEach(() => {
  pairing.creds = null;
  vi.unstubAllGlobals();
});

describe('getTelegramPhoneStatus', () => {
  it('reads GET /channels/telegram with the paired phone bearer', async () => {
    pairing.creds = { deviceToken: TOKEN };
    const fetchMock = vi.fn(async () => reply(200, { on: true, connected: true, botUsername: 'anna_pm_bot' }));
    vi.stubGlobal('fetch', fetchMock);

    const out = await getTelegramPhoneStatus(BASE);

    expect(out).toEqual({ on: true, connected: true, botUsername: 'anna_pm_bot' });
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe(`${BASE}/channels/telegram`);
    expect(init.method).toBe('GET');
    expect((init.headers as Record<string, string>).Authorization).toBe(`Bearer ${TOKEN}`);
  });

  it('reads a missing or odd field as off, with no bot name', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => reply(200, { on: 'yes', botUsername: 42 })));
    await expect(getTelegramPhoneStatus(BASE)).resolves.toEqual({ on: false, connected: false, botUsername: null });
  });

  it('sends no bearer when this phone is not paired', async () => {
    const fetchMock = vi.fn(async () => reply(200, { on: false, connected: false, botUsername: null }));
    vi.stubGlobal('fetch', fetchMock);
    await getTelegramPhoneStatus(BASE);
    const init = (fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1];
    expect((init.headers as Record<string, string>).Authorization).toBeUndefined();
  });

  it('adds the ngrok warning skip header only for an ngrok host', async () => {
    const fetchMock = vi.fn(async () => reply(200, { on: false, connected: false, botUsername: null }));
    vi.stubGlobal('fetch', fetchMock);
    await getTelegramPhoneStatus('https://pm-test.ngrok-free.app');
    const init = (fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1];
    expect((init.headers as Record<string, string>)['ngrok-skip-browser-warning']).toBe('1');
  });

  it('throws a message with the route and status only when the bridge refuses', async () => {
    pairing.creds = { deviceToken: TOKEN };
    vi.stubGlobal('fetch', vi.fn(async () => reply(403, { error: 'paired_phone_only' })));
    const err = await errorOf(getTelegramPhoneStatus(BASE));
    expect(err).toBeInstanceOf(Error);
    expect(err.message).toBe('GET /channels/telegram -> 403');
    expect(err.message).not.toContain(TOKEN);
  });
});

describe('turnTelegramOff', () => {
  it('POSTs /channels/telegram/off with the paired phone bearer and reads the result', async () => {
    pairing.creds = { deviceToken: TOKEN };
    const fetchMock = vi.fn(async () => reply(200, { ok: true, wasOn: true }));
    vi.stubGlobal('fetch', fetchMock);

    await expect(turnTelegramOff(BASE)).resolves.toEqual({ ok: true, wasOn: true });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe(`${BASE}/channels/telegram/off`);
    expect(init.method).toBe('POST');
    expect((init.headers as Record<string, string>).Authorization).toBe(`Bearer ${TOKEN}`);
  });

  it('reports wasOn false when Telegram was already off', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => reply(200, { ok: true, wasOn: false })));
    await expect(turnTelegramOff(BASE)).resolves.toEqual({ ok: true, wasOn: false });
  });

  it('throws on a refused call and never includes the body', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => reply(500, 'secret body text')));
    const err = await errorOf(turnTelegramOff(BASE));
    expect(err.message).toBe('POST /channels/telegram/off -> 500');
    expect(err.message).not.toContain('secret body text');
  });
});
