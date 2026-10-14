// @vitest-environment jsdom
// The app's local bridge key goes on its own loopback calls only, never to a tunnel or other host.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { isLoopbackBridgeUrl, localSecretHeaders, setBridgeLocalSecretForTest } from './bridge-local-secret';
import { makeBridgeClient } from './bridge';

vi.mock('./tauri', () => ({ tauri: { bridgeLocalSecret: vi.fn(async () => null) } }));

const KEY = 'ab'.repeat(32);

afterEach(() => {
  setBridgeLocalSecretForTest(null);
  vi.unstubAllGlobals();
});

describe('bridge local secret', () => {
  it('knows which bridge URLs are this machine', () => {
    expect(isLoopbackBridgeUrl('http://127.0.0.1:17321')).toBe(true);
    expect(isLoopbackBridgeUrl('http://localhost:17321')).toBe(true);
    expect(isLoopbackBridgeUrl('http://[::1]:17321')).toBe(true);
    expect(isLoopbackBridgeUrl('https://pm.example.com/bridge')).toBe(false);
    expect(isLoopbackBridgeUrl('https://abc.trycloudflare.com/bridge')).toBe(false);
    expect(isLoopbackBridgeUrl('not a url')).toBe(false);
  });

  it('adds the header only for a loopback bridge and only once the key is known', () => {
    expect(localSecretHeaders('http://127.0.0.1:17321')).toEqual({});
    setBridgeLocalSecretForTest(KEY);
    expect(localSecretHeaders('http://127.0.0.1:17321')).toEqual({ 'X-PM-Local-Secret': KEY });
    expect(localSecretHeaders('https://pm.example.com/bridge')).toEqual({});
  });

  it('the bridge client sends it on its calls to the local bridge', async () => {
    setBridgeLocalSecretForTest(KEY);
    const fetchMock = vi.fn(async () => new Response('[]', { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    await makeBridgeClient('http://127.0.0.1:17321').listPanes();
    const init = (fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1];
    expect((init.headers as Record<string, string>)['X-PM-Local-Secret']).toBe(KEY);

    fetchMock.mockClear();
    await makeBridgeClient('https://pm.example.com/bridge').listPanes();
    const remote = (fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1];
    expect(JSON.stringify(remote.headers ?? {})).not.toContain(KEY);
  });
});
