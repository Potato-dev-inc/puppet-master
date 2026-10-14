import { describe, expect, it } from 'vitest';
import { connectionLine } from './MobileSheet';

const link = (over: Partial<Parameters<typeof connectionLine>[0] & object> = {}) => ({
  mode: 'socket' as const,
  rttMs: 42,
  rttMedianMs: 40,
  reconnects: 0,
  ...over,
});

describe('connectionLine', () => {
  it('says nothing technical before anything is measured', () => {
    expect(connectionLine(null, null)).toBe('Connecting…');
    expect(connectionLine(link({ mode: 'stream' }), { lastMs: null, medianMs: null })).toBe('Connected on this network');
  });

  it('adds the typing delay once it is known', () => {
    expect(connectionLine(link({ mode: 'stream' }), { lastMs: 38, medianMs: 41 })).toBe(
      'Connected on this network · typing delay 38 ms (usually 41 ms)',
    );
  });

  it('describes a socket and its reconnects in plain words', () => {
    expect(connectionLine(link({ reconnects: 2 }), null)).toBe('Connected · 42 ms round trip · reconnected 2 times');
    expect(connectionLine(link({ reconnects: 1 }), null)).toContain('reconnected once');
  });

  it('never shows the old jargon or a bare dash', () => {
    for (const mode of ['stream', 'polling', 'connecting', 'socket'] as const) {
      const text = connectionLine(link({ mode }), null);
      expect(text).not.toMatch(/typing echo|median|RTT|—/);
    }
  });
});
