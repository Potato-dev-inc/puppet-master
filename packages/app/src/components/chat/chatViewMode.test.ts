// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { parseChatViewMode, readChatViewMode, writeChatViewMode } from './chatViewMode';

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe('parseChatViewMode', () => {
  it('keeps the three modes and makes anything else Normal', () => {
    expect(parseChatViewMode('summary')).toBe('summary');
    expect(parseChatViewMode('verbose')).toBe('verbose');
    expect(parseChatViewMode('loud')).toBe('normal');
    expect(parseChatViewMode(null)).toBe('normal');
  });
});

describe('the saved view mode of a pane', () => {
  it('is remembered per pane', () => {
    writeChatViewMode('pane-a', 'verbose');
    writeChatViewMode('pane-b', 'summary');
    expect(readChatViewMode('pane-a')).toBe('verbose');
    expect(readChatViewMode('pane-b')).toBe('summary');
    expect(readChatViewMode('pane-c')).toBe('normal');
  });

  it('is Normal and not saved without a pane', () => {
    writeChatViewMode(null, 'verbose');
    expect(readChatViewMode(null)).toBe('normal');
    expect(localStorage.length).toBe(0);
  });

  it('is Normal when storage throws, and a write that fails does not throw', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(readChatViewMode('pane-a')).toBe('normal');
    expect(() => writeChatViewMode('pane-a', 'summary')).not.toThrow();
  });
});
