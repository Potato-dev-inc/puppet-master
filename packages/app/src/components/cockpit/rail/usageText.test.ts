import { describe, expect, it } from 'vitest';
import type { HelperUsage, PaneUsage } from '../../../lib/pane-usage';
import { busyText, helperUsageLine, rollupLine, tokensText } from './usageText';

const helper = (over: Partial<HelperUsage> = {}): HelperUsage => ({ paneId: 'h', turns: 6, busyMs: 4 * 60_000, ...over });

describe('usage text', () => {
  it('writes busy time in plain minutes', () => {
    expect(busyText(0)).toBe('0 min');
    expect(busyText(30_000)).toBe('under 1 min');
    expect(busyText(4 * 60_000 + 20_000)).toBe('4 min');
    expect(busyText(65 * 60_000)).toBe('1 h 5 min');
  });

  it('writes the tokens the chat reported, in and out', () => {
    expect(tokensText({ input: 12_000, output: 3_000 })).toBe('12k in, 3k out tokens');
  });

  it('shows a helper line with account and no tokens when the chat reported none', () => {
    expect(helperUsageLine('Codex', helper({ accountLabel: 'work' }))).toBe('Codex · 6 turns · 4 min · work account');
  });

  it('adds tokens to the helper line only when they were reported', () => {
    const line = helperUsageLine('Codex', helper({ accountLabel: 'work', tokens: { input: 12_000, output: 3_000 } }));
    expect(line).toBe('Codex · 6 turns · 4 min · work account · 12k in, 3k out tokens');
    expect(helperUsageLine('Codex', helper({ tokens: null }))).not.toContain('tokens');
  });

  it('leaves out the account when the pane does not know it, and uses the singular for one turn', () => {
    expect(helperUsageLine('Codex', helper({ turns: 1, busyMs: 0 }))).toBe('Codex · 1 turn · 0 min');
  });

  it('writes the parent roll-up', () => {
    const usage: PaneUsage = {
      own: { turns: 2, busyMs: 0 },
      helperCount: 3,
      helperTurns: 14,
      helperBusyMs: 11 * 60_000,
      helpers: [],
    };
    expect(rollupLine(usage)).toBe('Helpers: 3 · 14 turns · 11 min');
  });
});
