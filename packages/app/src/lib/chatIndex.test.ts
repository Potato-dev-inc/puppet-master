import { describe, expect, it } from 'vitest';
import { formatBytes, indexSizeText, indexStatusText } from './chatIndex';

describe('formatBytes', () => {
  it('uses one short figure with a unit', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(812)).toBe('812 B');
    expect(formatBytes(96 * 1024)).toBe('96 KB');
    expect(formatBytes(2_202_010)).toBe('2.1 MB');
    expect(formatBytes(3 * 1024 ** 3)).toBe('3.0 GB');
  });
});

describe('indexSizeText', () => {
  it('shows the size, then the items with separators', () => {
    expect(indexSizeText({ items: 4312, bytes: 2_202_010 })).toBe('2.1 MB · 4,312 items');
    expect(indexSizeText({ items: 1, bytes: 512 })).toBe('512 B · 1 item');
    expect(indexSizeText({ items: 0, bytes: 0 })).toBe('0 B · 0 items');
  });
});

describe('indexStatusText', () => {
  it('is the size line when there is no old copy', () => {
    expect(indexStatusText({ items: 1, bytes: 2048 })).toBe(indexSizeText({ items: 1, bytes: 2048 }));
    expect(indexStatusText({ items: 1, bytes: 2048, oldCopies: 0 })).toBe('2.0 KB · 1 item');
  });

  it('says how many old damaged copies are left, in the singular and the plural', () => {
    expect(indexStatusText({ items: 3, bytes: 2048, oldCopies: 1 })).toBe('2.0 KB · 3 items · 1 old damaged copy (removed too)');
    expect(indexStatusText({ items: 3, bytes: 2048, oldCopies: 2 })).toBe('2.0 KB · 3 items · 2 old damaged copies (removed too)');
  });
});
