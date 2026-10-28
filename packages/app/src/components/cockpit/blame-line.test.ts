import { describe, expect, it } from 'vitest';
import { blameLine, clockTime } from './blame-line';

describe('blameLine', () => {
  const at = new Date(2026, 9, 28, 14, 2).getTime();

  it('shows the agent, the pane title and the local time', () => {
    const line = blameLine({ agent: 'Codex', paneTitle: 'helper', atMs: at });
    expect(line.text).toBe('Codex helper \u00b7 14:02');
    expect(line.title).toBe('Changed by Codex in turn at 14:02');
  });

  it('shows only the agent when the pane has no title', () => {
    expect(blameLine({ agent: 'Claude Code', paneTitle: null, atMs: at }).text).toBe('Claude Code \u00b7 14:02');
  });

  it('pads the clock to two digits', () => {
    expect(clockTime(new Date(2026, 0, 1, 9, 5).getTime())).toBe('09:05');
  });
});
