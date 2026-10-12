import { describe, expect, it } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { AgentDescriptor } from './agentRegistry';
import { ago, orderPanes, phoneLaunchEntries, previewLines, projectName, statusMeta, tabLabels } from './mobile-agent-tabs';

function pane(id: string, agent_type: string, created_at: number): PaneInfo {
  return { id, agent_type, created_at, pid: 1, status: 'idle', last_output_at: null, cwd: 'C:\\w\\proj', cols: 80, rows: 24 };
}

describe('mobile agent tabs', () => {
  it('orders panes oldest first and numbers repeated agents', () => {
    const panes = [pane('c', 'claude', 30), pane('a', 'claude', 10), pane('k', 'kilo', 20)];
    expect(orderPanes(panes).map((p) => p.id)).toEqual(['a', 'k', 'c']);
    const labels = tabLabels(panes);
    expect(labels.get('a')).toBe('claude 1');
    expect(labels.get('c')).toBe('claude 2');
    expect(labels.get('k')).toBe('kilo');
  });

  it('names the project folder', () => {
    expect(projectName('C:\\Users\\me\\puppet-master\\')).toBe('puppet-master');
    expect(projectName('/home/me/app')).toBe('app');
  });

  it('flags panes that need the user', () => {
    expect(statusMeta('waiting_input')).toMatchObject({ label: 'needs you', attention: true });
    expect(statusMeta('running')).toMatchObject({ label: 'working', pulse: true, attention: false });
    expect(statusMeta('something_new').label).toBe('something new');
  });

  it('formats output age', () => {
    expect(ago(null, 1000)).toBeNull();
    expect(ago(1000, 3000)).toBe('just now');
    expect(ago(0, 42_000)).toBe('42s ago');
    expect(ago(0, 5 * 60_000)).toBe('5m ago');
    expect(ago(0, 3 * 3_600_000)).toBe('3h ago');
  });

  it('previews the last meaningful screen lines without frames', () => {
    const screen = [
      '  ┃  Reply with one word: PINEAPPLE.',
      '  ┃',
      '     PINEAPPLE',
      '     ▣  Code · Auto Free · 8.4s',
      '  ╹▀▀▀▀▀▀▀▀▀▀',
      '                 ',
    ].join('\n');
    expect(previewLines(screen)).toEqual(['PINEAPPLE', '▣ Code · Auto Free · 8.4s']);
    expect(previewLines('', 2)).toEqual([]);
  });

  it('skips the input box and footer at the bottom of the screen', () => {
    const kilo = [
      '     PINEAPPLE',
      '     ▣  Code · Auto Free · 5.3s',
      '',
      '',
      '',
      '  ┃',
      '  ┃  Code  · Auto Free Kilo Gateway',
      '  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀',
      '                     21.2K (8%)  ctrl+p commands',
      '',
    ].join('\n');
    expect(previewLines(kilo)).toEqual(['PINEAPPLE', '▣ Code · Auto Free · 5.3s']);
    const claude = ['● refactor/tui-profiles', '', '────────────', '> ', '────────────', '  ? for shortcuts'].join('\n');
    expect(previewLines(claude, 1)).toEqual(['● refactor/tui-profiles']);
    // Nothing above the gap: the bottom lines are all there is, so they are shown.
    expect(previewLines(['', '', 'Welcome to the agent'].join('\n'))).toEqual(['Welcome to the agent']);
  });

  it('offers visible, installed agents first and the rest disabled with a reason', () => {
    const agent = (id: string, visible: boolean, installed: boolean) =>
      ({ id, label: id.toUpperCase(), icon: null, modes: { visible, headless: true }, installed, experimental: false }) as unknown as AgentDescriptor;
    const entries = phoneLaunchEntries({
      agents: [agent('kiro', false, true), agent('claude', true, true), agent('codex', true, false)],
      dispatchable_ids: [],
      source: 'registry',
    });
    expect(entries.map((e) => [e.type, e.disabledReason ?? null])).toEqual([
      ['claude', null],
      ['kiro', 'Headless only'],
      ['codex', 'Not installed'],
    ]);
  });
});
