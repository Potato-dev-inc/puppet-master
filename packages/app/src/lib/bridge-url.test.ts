import { describe, expect, it } from 'vitest';
import { resolveBridgeBaseUrl, shouldUseSameOriginBridgeProxy } from './bridge-url';

describe('shouldUseSameOriginBridgeProxy', () => {
  it('uses proxy for https origins', () => {
    expect(
      shouldUseSameOriginBridgeProxy({
        protocol: 'https:',
        hostname: 'pm.example.com',
        origin: 'https://pm.example.com',
      }),
    ).toBe(true);
  });

  it('uses proxy for http remote hosts', () => {
    expect(
      shouldUseSameOriginBridgeProxy({
        protocol: 'http:',
        hostname: 'pm.example.com',
        origin: 'http://pm.example.com',
      }),
    ).toBe(true);
  });

  it('does not use proxy on local dev', () => {
    expect(
      shouldUseSameOriginBridgeProxy({
        protocol: 'http:',
        hostname: '127.0.0.1',
        origin: 'http://127.0.0.1:1420',
      }),
    ).toBe(false);
  });
});

describe('resolveBridgeBaseUrl', () => {
  const tunnel = {
    protocol: 'https:' as const,
    hostname: 'pm.example.com',
    origin: 'https://pm.example.com',
  };

  it('defaults to same-origin proxy on tunneled https', () => {
    expect(resolveBridgeBaseUrl(null, tunnel)).toBe('https://pm.example.com/bridge');
  });

  it('rewrites saved localhost URL when opened via tunnel', () => {
    expect(resolveBridgeBaseUrl('http://127.0.0.1:17321', tunnel)).toBe(
      'https://pm.example.com/bridge',
    );
  });

  it('keeps explicit remote bridge URL', () => {
    expect(resolveBridgeBaseUrl('https://bridge.example.com', tunnel)).toBe(
      'https://bridge.example.com',
    );
  });

  it('defaults to local bridge on loopback dev', () => {
    const local = {
      protocol: 'http:' as const,
      hostname: '127.0.0.1',
      origin: 'http://127.0.0.1:1420',
    };
    expect(resolveBridgeBaseUrl(null, local)).toBe('http://127.0.0.1:17321');
  });
});
