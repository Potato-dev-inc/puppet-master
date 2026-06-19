import { describe, expect, it } from 'vitest';
import { resolveBridgeBaseUrl } from './bridge-url.js';

describe('resolveBridgeBaseUrl', () => {
  it('uses same-origin /bridge on tunnel hosts even when localStorage has another domain', () => {
    const tunnel = {
      origin: 'https://rid-navy-div-breeding.trycloudflare.com',
      protocol: 'https:',
      hostname: 'rid-navy-div-breeding.trycloudflare.com',
    };
    expect(
      resolveBridgeBaseUrl('https://3001.v7ren.xyz/bridge', tunnel),
    ).toBe('https://rid-navy-div-breeding.trycloudflare.com/bridge');
  });

  it('keeps saved URL when it matches the current origin', () => {
    const loc = {
      origin: 'https://3001.v7ren.xyz',
      protocol: 'https:',
      hostname: '3001.v7ren.xyz',
    };
    expect(resolveBridgeBaseUrl('https://3001.v7ren.xyz/bridge', loc)).toBe(
      'https://3001.v7ren.xyz/bridge',
    );
  });
});
