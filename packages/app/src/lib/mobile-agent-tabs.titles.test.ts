import { describe, expect, it } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import { tabLabels } from './mobile-agent-tabs';

function pane(id: string, agent_type: string, created_at: number, title?: string | null): PaneInfo {
  return { id, agent_type, created_at, pid: 1, status: 'idle', last_output_at: null, cwd: 'C:\w\proj', cols: 80, rows: 24, title } as PaneInfo;
}

describe('mobile agent tabs with pane titles', () => {
  it('uses the trimmed pane title as the tab label', () => {
    const labels = tabLabels([pane('a', 'claude', 10, '  auth refactor  ')]);
    expect(labels.get('a')).toBe('auth refactor');
  });

  it('replaces the numbered agent label for titled panes and keeps numbering the untitled ones', () => {
    const panes = [pane('a', 'claude', 10, 'auth refactor'), pane('b', 'claude', 20), pane('c', 'claude', 30, null)];
    const labels = tabLabels(panes);
    expect(labels.get('a')).toBe('auth refactor');
    expect(labels.get('b')).toBe('Claude Code 2');
    expect(labels.get('c')).toBe('Claude Code 3');
  });

  it('uses the plain agent id for an untitled pane that runs alone and has no known label', () => {
    expect(tabLabels([pane('k', 'kilo', 10, undefined)]).get('k')).toBe('kilo');
  });

  it('ignores whitespace-only titles', () => {
    const labels = tabLabels([pane('a', 'claude', 10, '   '), pane('b', 'claude', 20, '\t')]);
    expect(labels.get('a')).toBe('Claude Code 1');
    expect(labels.get('b')).toBe('Claude Code 2');
  });

  it('a pane title beats the display name, even when the display name is numbered', () => {
    const labels = tabLabels([pane('a', 'powershell', 10, 'deploy shell'), pane('b', 'powershell', 20)]);
    expect(labels.get('a')).toBe('deploy shell');
    expect(labels.get('b')).toBe('PowerShell 2');
  });
});
