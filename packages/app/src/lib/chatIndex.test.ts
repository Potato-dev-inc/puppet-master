import { describe, expect, it } from 'vitest';
import { formatBytes, indexSizeText } from './chatIndex';

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
