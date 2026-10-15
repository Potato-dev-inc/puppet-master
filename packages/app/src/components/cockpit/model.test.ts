import { describe, expect, it } from 'vitest';
import type { PaneData } from '../../hooks/usePaneRegistry';
import { formatElapsed, groupPanes, matchesQuery, paneNames, receiptRows, spawnLogLine, spinnerStatus, stateText } from './model';

function pane(id: string, agent: string, status: string): PaneData {
  return {
    status,
    info: { id, agent_type: agent, status, pid: 1, created_at: 1, last_output_at: null, cwd: 'C:\\x', cols: 80, rows: 24 },
  } as unknown as PaneData;
}

describe('cockpit model', () => {
  it('groups by state', () => {
    const g = groupPanes([pane('a', 'claude', 'waiting_input'), pane('b', 'codex', 'error'), pane('c', 'x', 'running'), pane('d', 'y', 'idle')]);
    expect(g.needs.map((p) => p.info.id)).toEqual(['a', 'b']);
    expect(g.working.map((p) => p.info.id)).toEqual(['c']);
    expect(g.idle.map((p) => p.info.id)).toEqual(['d']);
  });
  it('puts a pane with an open agent question under Needs you', () => {
    const list = [pane('a', 'claude', 'running'), pane('b', 'codex', 'idle')];
    expect(groupPanes(list).needs).toEqual([]);
    const g = groupPanes(list, new Set(['a']));
    expect(g.needs.map((p) => p.info.id)).toEqual(['a']);
    expect(g.working).toEqual([]);
    expect(g.idle.map((p) => p.info.id)).toEqual(['b']);
  });
  it('formats elapsed', () => {
    expect(formatElapsed(4000)).toBe('4s');
    expect(formatElapsed(125_000)).toBe('2m');
    expect(formatElapsed(3_900_000)).toBe('1h 5m');
  });
  it('numbers duplicate agents', () => {
    const names = paneNames(
      [pane('a', 'claude', 'idle'), pane('b', 'claude', 'idle'), pane('c', 'codex', 'idle')],
      (id) => id.toUpperCase(),
    );
    expect(names.get('a')).toBe('CLAUDE 1');
    expect(names.get('b')).toBe('CLAUDE 2');
    expect(names.get('c')).toBe('CODEX');
  });
  it('maps unknown status to exited', () => {
    expect(spinnerStatus('running')).toBe('running');
    expect(spinnerStatus('weird')).toBe('exited');
  });
  it('matches words', () => {
    expect(matchesQuery('Switch to Claude Code', 'claude sw')).toBe(true);
    expect(matchesQuery('Interrupt', 'claude')).toBe(false);
  });
  it('names a pane state in plain words', () => {
    expect(stateText('running')).toBe('Working');
    expect(stateText('waiting_input')).toBe('Needs you');
    expect(stateText('idle')).toBe('Idle');
    expect(stateText('error')).toBe('Error');
    expect(stateText('exited')).toBe('Exited');
  });
  it('spawn log prefers backend message', () => {
    expect(spawnLogLine({ agent_id: 'a', pane_id: null, stage: 'claiming', warm: true, message: 'hi' })).toBe('hi');
    expect(spawnLogLine({ agent_id: 'a', pane_id: null, stage: 'ready', warm: false })).toBe('Ready');
  });
  it('spawn log uses plain words for all five stages, with and without an agent name', () => {
    const stage = (s: 'claiming' | 'spawning' | 'attached' | 'ready' | 'failed', warm = false) =>
      ({ agent_id: 'claude', pane_id: null, stage: s, warm }) as Parameters<typeof spawnLogLine>[0];
    expect(spawnLogLine(stage('claiming'), 'Claude Code')).toBe('Getting a ready agent');
    expect(spawnLogLine(stage('spawning'), 'Claude Code')).toBe('Starting Claude Code');
    expect(spawnLogLine(stage('spawning', true), 'Claude Code')).toBe('Starting Claude Code');
    expect(spawnLogLine(stage('spawning'))).toBe('Starting the agent');
    expect(spawnLogLine(stage('attached'))).toBe('Connected');
    expect(spawnLogLine(stage('ready'))).toBe('Ready');
    expect(spawnLogLine(stage('failed'))).toBe('Could not start');
    const all = [
      spawnLogLine(stage('claiming')),
      spawnLogLine(stage('spawning')),
      spawnLogLine(stage('spawning'), 'Codex'),
      spawnLogLine(stage('attached')),
      spawnLogLine(stage('ready')),
      spawnLogLine(stage('failed')),
    ];
    for (const line of all) {
      for (const jargon of ['warm', 'standby', 'PTY', 'process', 'attached']) {
        expect(line).not.toContain(jargon);
      }
    }
  });
  it('flattens receipts', () => {
    expect(receiptRows({ a: 1, b: { c: 'x' }, d: [1, 2] })).toEqual([
      { key: 'a', value: '1' },
      { key: 'b.c', value: 'x' },
      { key: 'd', value: '1, 2' },
    ]);
  });
});
