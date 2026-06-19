import { describe, expect, it } from 'vitest';
import {
  bridgeRequiresPairingToken,
  buildPairingInviteUrl,
  encodePairingQrContent,
  encodePairingQrPayload,
  pairResponseMessage,
  parsePairingInviteFromQr,
  parsePairingInvitePath,
  parsePairingQrPayload,
  PM_PROXIED_HEADER,
} from './mobile-pairing.js';

describe('mobile-pairing', () => {
  it('round-trips QR payload', () => {
    const payload = {
      v: 1 as const,
      u: 'https://abc.trycloudflare.com/bridge',
      pk: 'dGVzdC1wdWJsaWMta2V5LTMyLWJ5dGVzISE=',
      c: 'ABCD1234',
      e: 1_700_000_000,
    };
    const parsed = parsePairingQrPayload(encodePairingQrPayload(payload));
    expect(parsed).toEqual(payload);
  });

  it('builds invite URLs for camera-friendly QRs', () => {
    expect(buildPairingInviteUrl('https://3001.v7ren.xyz', 'PM8K2X4Q')).toBe(
      'https://3001.v7ren.xyz/pair/PM8K2X4Q',
    );
    expect(encodePairingQrContent(
      { v: 1, u: 'https://x/bridge', pk: 'pk', c: 'CODE12', e: 99 },
      'https://3001.v7ren.xyz',
    )).toBe('https://3001.v7ren.xyz/pair/CODE12');
  });

  it('parses invite paths and scanned URLs', () => {
    expect(parsePairingInvitePath('/pair/PM8K2X4Q')).toBe('PM8K2X4Q');
    expect(parsePairingInviteFromQr('https://3001.v7ren.xyz/pair/pm8k2x4q')).toBe('PM8K2X4Q');
  });

  it('detects proxied bridge requests', () => {
    expect(bridgeRequiresPairingToken({ [PM_PROXIED_HEADER]: '1' })).toBe(true);
    expect(bridgeRequiresPairingToken({})).toBe(false);
  });

  it('builds pair response message', () => {
    expect(pairResponseMessage('id', 'tok', 'pub')).toBe('id|tok|pub');
  });
});
