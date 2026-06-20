import { describe, expect, it } from 'vitest';
import { MirrorEchoFilter } from './mirror-echo-filter';

describe('MirrorEchoFilter', () => {
  it('skips inbound bytes that match outbound echo', () => {
    const filter = new MirrorEchoFilter();
    filter.noteOutbound('cou');
    expect(filter.shouldSkipInbound(new TextEncoder().encode('cou'))).toBe(true);
    expect(filter.shouldSkipInbound(new TextEncoder().encode('cou'))).toBe(false);
  });

  it('consumes echo incrementally', () => {
    const filter = new MirrorEchoFilter();
    filter.noteOutbound('abc');
    expect(filter.shouldSkipInbound(new TextEncoder().encode('a'))).toBe(true);
    expect(filter.shouldSkipInbound(new TextEncoder().encode('bc'))).toBe(true);
  });

  it('skips replacement backspace echoes separately from replacement text', () => {
    const filter = new MirrorEchoFilter();
    filter.noteOutbound('sufh');
    filter.noteOutbound('\x7f\x7f\x7f\x7fmuch');

    expect(filter.shouldSkipInbound(new TextEncoder().encode('\b \b\b \b\b \b\b \b'))).toBe(true);
    expect(filter.shouldSkipInbound(new TextEncoder().encode('much'))).toBe(true);
  });

  it('passes through unrelated output', () => {
    const filter = new MirrorEchoFilter();
    filter.noteOutbound('ls');
    expect(filter.shouldSkipInbound(new TextEncoder().encode('output'))).toBe(false);
  });

  it('tracks backspace without pending chars for PTY echo dedupe', () => {
    const filter = new MirrorEchoFilter();
    expect(filter.noteBackspaceForEcho()).toBe('');
    expect(filter.shouldSkipInbound(new TextEncoder().encode('\b \b'))).toBe(false);
  });

  it('locally erases after printable echo was deduped', () => {
    const filter = new MirrorEchoFilter();
    filter.noteOutbound('你');
    expect(filter.shouldSkipInbound(new TextEncoder().encode('你'))).toBe(true);
    expect(filter.noteBackspaceForEcho()).toBe('\b \b\b \b');
    expect(filter.shouldSkipInbound(new TextEncoder().encode('\b \b\b \b'))).toBe(true);
  });
});
