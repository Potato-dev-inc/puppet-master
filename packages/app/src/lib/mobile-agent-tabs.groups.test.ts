import { describe, expect, it } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import { glyphStatus, groupPanes, waitingCount } from './mobile-agent-tabs';

const pane = (id: string, status: string, created_at = 0) =>
  ({ id, status, created_at, agent_type: 'claude', cwd: 'C:/x', pid: 1, last_output_at: null, cols: 80, rows: 24 }) as unknown as PaneInfo;

describe('glyphStatus', () => {
  it('keeps running, waiting_input and error', () => {
    expect(glyphStatus('running')).toBe('running');
    expect(glyphStatus('waiting_input')).toBe('waiting_input');
    expect(glyphStatus('error')).toBe('error');
  });

  it('reads every other status as idle', () => {
    expect(glyphStatus('idle')).toBe('idle');
    expect(glyphStatus('exited')).toBe('idle');
    expect(glyphStatus('weird')).toBe('idle');
  });
});

describe('groupPanes', () => {
  it('puts waiting and error panes under Needs you, running under Working, the rest under Idle', () => {
    const groups = groupPanes([
      pane('a', 'idle', 1),
      pane('b', 'running', 2),
      pane('c', 'waiting_input', 3),
      pane('d', 'error', 4),
      pane('e', 'exited', 5),
    ]);
    expect(groups.map((g) => [g.id, g.title])).toEqual([
      ['needs', 'Needs you'],
      ['working', 'Working'],
      ['idle', 'Idle'],
    ]);
    expect(groups[0].panes.map((p) => p.id)).toEqual(['c', 'd']);
    expect(groups[1].panes.map((p) => p.id)).toEqual(['b']);
    expect(groups[2].panes.map((p) => p.id)).toEqual(['a', 'e']);
  });

  it('keeps input order inside a group', () => {
    const groups = groupPanes([pane('x', 'idle'), pane('y', 'idle'), pane('z', 'idle')]);
    expect(groups).toHaveLength(1);
    expect(groups[0].panes.map((p) => p.id)).toEqual(['x', 'y', 'z']);
  });

  it('omits empty groups and returns no groups for no panes', () => {
    expect(groupPanes([])).toEqual([]);
    const groups = groupPanes([pane('w', 'running'), pane('i', 'idle')]);
    expect(groups.map((g) => g.id)).toEqual(['working', 'idle']);
  });
});

describe('waitingCount', () => {
  it('is 00 when nothing needs the user', () => {
    expect(waitingCount([])).toBe('00');
    expect(waitingCount([pane('a', 'running'), pane('b', 'idle')])).toBe('00');
  });

  it('counts waiting and error panes, zero-padded to two digits', () => {
    expect(waitingCount([pane('a', 'waiting_input'), pane('b', 'error'), pane('c', 'running')])).toBe('02');
  });
});
