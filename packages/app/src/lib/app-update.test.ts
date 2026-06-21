import { describe, expect, it } from 'vitest';
import { isVersionNewer, normalizeVersion, parseVersionParts } from './app-update';

describe('app-update', () => {
  it('normalizes leading v', () => {
    expect(normalizeVersion('v0.1.2')).toBe('0.1.2');
  });

  it('parses dotted versions', () => {
    expect(parseVersionParts('1.2.3')).toEqual([1, 2, 3]);
  });

  it('detects newer patch versions', () => {
    expect(isVersionNewer('0.1.3', '0.1.2')).toBe(true);
    expect(isVersionNewer('0.1.2', '0.1.2')).toBe(false);
    expect(isVersionNewer('0.1.1', '0.1.2')).toBe(false);
  });

  it('detects newer minor versions', () => {
    expect(isVersionNewer('0.2.0', '0.1.9')).toBe(true);
  });
});
