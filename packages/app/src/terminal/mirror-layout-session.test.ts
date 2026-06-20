import { describe, expect, it } from 'vitest';
import { mirrorLayoutSessionKey } from './mirror-layout-session';

describe('mirrorLayoutSessionKey', () => {
  it('returns a stable key for the same inputs', () => {
    const key = mirrorLayoutSessionKey(1_700_000_000_000, 80, 24);
    expect(mirrorLayoutSessionKey(1_700_000_000_000, 80, 24)).toBe(key);
  });

  it('changes when cols change', () => {
    const base = mirrorLayoutSessionKey(1_000, 80, 24);
    expect(mirrorLayoutSessionKey(1_000, 120, 24)).not.toBe(base);
  });

  it('changes when rows change', () => {
    const base = mirrorLayoutSessionKey(1_000, 80, 24);
    expect(mirrorLayoutSessionKey(1_000, 80, 40)).not.toBe(base);
  });

  it('changes when createdAt changes', () => {
    const base = mirrorLayoutSessionKey(1_000, 80, 24);
    expect(mirrorLayoutSessionKey(2_000, 80, 24)).not.toBe(base);
  });
});
