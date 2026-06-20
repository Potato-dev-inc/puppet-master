import { describe, expect, it } from 'vitest';
import {
  localDevBridgeUrl,
  normalizePublicBridgeUrl,
  resolvePairingBridgeUrl,
} from './public-bridge-url.js';

describe('public-bridge-url', () => {
  it('normalizes custom domains to /bridge', () => {
    expect(normalizePublicBridgeUrl('https://3001.v7ren.com')).toBe('https://3001.v7ren.com/bridge');
    expect(normalizePublicBridgeUrl('https://pm.example.com/bridge')).toBe('https://pm.example.com/bridge');
  });

  it('prefers saved custom URL over tunnel dev info', () => {
    const url = resolvePairingBridgeUrl(
      { bridgeProxyUrl: 'https://random.trycloudflare.com/bridge' },
      'https://3001.v7ren.xyz',
    );
    expect(url).toBe('https://3001.v7ren.xyz/bridge');
  });

  it('falls back to configured dev server port', () => {
    expect(localDevBridgeUrl(3001)).toBe('http://127.0.0.1:3001/bridge');
    expect(resolvePairingBridgeUrl(null, null, 3001)).toBe('http://127.0.0.1:3001/bridge');
  });

  it('prefers cloudflare tunnel over loopback bridgeProxyUrl from desktop defaults', () => {
    const url = resolvePairingBridgeUrl(
      {
        bridgeProxyUrl: 'http://127.0.0.1:1420/bridge',
        tunnelUrl: 'https://abc.trycloudflare.com',
      },
      null,
    );
    expect(url).toBe('https://abc.trycloudflare.com/bridge');
  });

  it('ignores loopback bridgeProxyUrl when no tunnelUrl', () => {
    const url = resolvePairingBridgeUrl(
      { bridgeProxyUrl: 'http://127.0.0.1:1420/bridge' },
      null,
      3001,
    );
    expect(url).toBe('http://127.0.0.1:3001/bridge');
  });
});
