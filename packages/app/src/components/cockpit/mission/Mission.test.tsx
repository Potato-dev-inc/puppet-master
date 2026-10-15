// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DETECTED, MISSION_DONE, MISSION_STUCK, MISSION_TAMPER, MISSION_WORKING } from '../../../dev/mission-fixtures';
import type { MissionView, MissionsApi } from '../../../lib/missions';
import { MissionCard, MissionsSection } from './MissionCards';
import { MissionProgress } from './MissionProgress';
import { StartMissionSheet } from './StartMissionSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  disconnect() {}
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function fakeApi(over: Partial<MissionsApi> = {}): MissionsApi & { calls: string[] } {
  const calls: string[] = [];
  const echo = (m: MissionView) => Promise.resolve(m);
  return {
    calls,
    list: async () => [],
    get: async (id) => ({ ...MISSION_WORKING, id, notes: '- (Codex, turn 5) Rounding fixed.' }),
    detect: async () => DETECTED,
    start: vi.fn(async () => MISSION_WORKING),
    stop: async (id) => {
      calls.push(`stop:${id}`);
      return echo({ ...MISSION_WORKING, state: 'stopped' });
    },
    pause: async (id) => {
      calls.push(`pause:${id}`);
      return echo(MISSION_WORKING);
    },
    resume: async (id) => {
      calls.push(`resume:${id}`);
      return echo(MISSION_WORKING);
    },
    decide: async (id, choice) => {
      calls.push(`decide:${id}:${choice}`);
      return echo(MISSION_DONE);
    },
    ...over,
  };
}

const click = (el: Element | null | undefined) => act(() => (el as HTMLElement).click());
const button = (text: string) => [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
const flush = () => act(async () => {});

function type(selector: string, value: string) {
  const field = document.querySelector(selector) as HTMLTextAreaElement;
  act(() => {
    const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
    set.call(field, value);
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

/** UX 0.3: raw words never appear in mission chrome. */
function noRawWords() {
  const text = (document.body.textContent ?? '').toLowerCase();
  for (const word of ['tamper', 'worktree', 'rounds', 'round 1', 'operation', 'mission-m', 'budget_', 'mission_']) {
    expect(text.includes(word), `"${word}" in: ${text}`).toBe(false);
  }
}

describe('the tamper card', () => {
  it('lists the test changes, blocks done and offers Undo first', () => {
    act(() =>
      root.render(
        <ul>
          <MissionCard m={MISSION_TAMPER} api={fakeApi()} />
        </ul>,
      ),
    );
    expect(host.textContent).toContain('The checks pass, but Codex changed the tests');
    expect(host.textContent).toContain('Added .skip to "handles 0.005" in cart.test.ts');
    expect(host.textContent).toContain("The mission isn't done until you decide.");
    const buttons = [...host.querySelectorAll('.ck-inbox-actions button')].map((b) => b.textContent);
    expect(buttons).toEqual(['Undo the test changes and keep going', 'Accept the changes']);
    expect(host.querySelector('.ck-btn--primary')?.textContent).toBe('Undo the test changes and keep going');
    expect(host.querySelector('.ck-preview-tag')?.textContent).toBe('Preview');
    noRawWords();
  });

  it('Undo the test changes is one click; Accept asks first', async () => {
    const api = fakeApi();
    act(() =>
      root.render(
        <ul>
          <MissionCard m={MISSION_TAMPER} api={api} />
        </ul>,
      ),
    );
    click(button('Undo the test changes and keep going'));
    await flush();
    expect(api.calls).toEqual(['decide:m2:undo_tests']);
    click(button('Accept the changes'));
    expect(document.body.textContent).toContain('Count these tests as passing?');
    expect(api.calls, 'nothing is accepted before the confirm').toEqual(['decide:m2:undo_tests']);
    click(button('Accept and finish'));
    await flush();
    expect(api.calls).toEqual(['decide:m2:undo_tests', 'decide:m2:accept_tests']);
  });
});

describe('the stuck card', () => {
  it('offers more turns, hand it to me, or stop', async () => {
    const api = fakeApi();
    act(() =>
      root.render(
        <ul>
          <MissionCard m={MISSION_STUCK} api={api} />
        </ul>,
      ),
    );
    expect(host.textContent).toContain("The mission isn't getting anywhere");
    expect(button('Try 5 more turns')).toBeDefined();
    click(button('Hand it to me'));
    await flush();
    expect(api.calls).toEqual(['decide:m4:hand_to_me']);
  });
});

describe('Progress', () => {
  it('shows the state, the budget, one bar and a row per turn, newest first', () => {
    act(() => root.render(<MissionProgress mission={MISSION_WORKING} api={fakeApi()} />));
    expect(host.textContent).toContain('Turn 7 · 9 of 12 pass');
    expect(host.textContent).toContain('7 of 30 turns · 41 of 120 min');
    expect(host.querySelectorAll('[role="progressbar"]').length).toBe(1);
    const rows = [...host.querySelectorAll('.ck-mission-row')].map((r) => r.textContent ?? '');
    expect(rows[0]).toContain('Turn 7');
    expect(rows.some((r) => r.includes("went back to turn 5's files"))).toBe(true);
    expect(rows.some((r) => r.includes('Handed to Codex'))).toBe(true);
    expect(host.querySelector('.ck-preview-tag')).not.toBeNull();
    noRawWords();
  });

  it('pauses after this turn and stops only after a confirm', async () => {
    const api = fakeApi();
    act(() => root.render(<MissionProgress mission={MISSION_WORKING} api={api} />));
    click(button('Pause after this turn'));
    await flush();
    expect(api.calls).toEqual(['pause:m1']);
    click(button('Stop mission…'));
    expect(document.body.textContent).toContain('Stop this mission?');
    expect(document.body.textContent).toContain('Its separate copy and notes stay');
    click(button('Stop mission'));
    await flush();
    expect(api.calls).toEqual(['pause:m1', 'stop:m1']);
  });

  it('a finished mission shows the Done review and says nothing was merged', () => {
    act(() => root.render(<MissionProgress mission={MISSION_DONE} api={fakeApi()} onOpenPane={() => {}} />));
    expect(host.textContent).toContain('Done · 12 of 12 pass · ready to review');
    expect(host.textContent).toContain('Nothing has been merged.');
    expect(button('Review changes')).toBeDefined();
    expect(button('Pause after this turn')).toBeUndefined();
    expect(button('Merge')).toBeUndefined();
  });

  it('Notes shows the agents notes', async () => {
    act(() => root.render(<MissionProgress mission={MISSION_WORKING} api={fakeApi()} />));
    click([...host.querySelectorAll('[role="tab"]')].find((t) => t.textContent === 'Notes'));
    await flush();
    expect(host.textContent).toContain('Rounding fixed.');
  });
});

describe('the Inbox section', () => {
  it('puts cards first, then the missions, finished ones under Ready for you', () => {
    act(() => root.render(<MissionsSection missions={[MISSION_TAMPER, MISSION_WORKING, MISSION_DONE]} api={fakeApi()} />));
    expect(host.querySelectorAll('[data-mission-card]').length).toBe(1);
    const caps = [...host.querySelectorAll('.ck-mission-cap')].map((c) => c.textContent);
    expect(caps).toEqual(['Missions', 'Ready for you']);
    // Every mission surface carries the Preview tag: the card and each row.
    expect(host.querySelectorAll('.ck-preview-tag').length).toBe(4);
  });

  it('renders nothing without missions', () => {
    act(() => root.render(<MissionsSection missions={[]} api={fakeApi()} />));
    expect(host.textContent).toBe('');
  });
});

describe('the start sheet', () => {
  const entries = [
    { type: 'claude', label: 'Claude Code' },
    { type: 'codex', label: 'Codex' },
  ];

  it('is disabled with a reason until there is a goal, then starts with the checks that are on', async () => {
    const api = fakeApi();
    const started = vi.fn();
    act(() =>
      root.render(
        <StartMissionSheet
          project={{ path: 'C:\\work\\shop', name: 'shop', git: true, branch: 'main' }}
          entries={entries}
          activeMissions={0}
          api={api}
          onClose={() => {}}
          onStarted={started}
        />,
      ),
    );
    await flush();
    expect((button('Start mission') as HTMLButtonElement).disabled).toBe(true);
    expect(document.body.textContent).toContain("Say what should be true when it's done.");
    expect(document.body.textContent).toContain('npx tsc --noEmit');
    expect(document.body.textContent).toContain('Preview');
    expect(document.body.textContent).toContain('shop · in a separate copy from main. You review and merge at the end.');
    expect(document.body.textContent).toContain('Every agent works in a visible pane');
    type('#ck-mission-goal', 'Fix the cart tests');
    expect(document.body.textContent).toContain('Your goal mentions tests, so agents may change them.');
    expect((button('Start mission') as HTMLButtonElement).disabled).toBe(false);
    click(button('Start mission'));
    await flush();
    expect(api.start).toHaveBeenCalledWith(
      expect.objectContaining({
        project_path: 'C:\\work\\shop',
        objective: 'Fix the cart tests',
        agents: ['claude'],
        may_change_tests: true,
        rollback: true,
        budget_turns: 30,
        budget_minutes: 120,
        checks: [
          { name: 'Tests', command: ['npm', 'test'] },
          { name: 'Type check', command: ['npx', 'tsc', '--noEmit'] },
        ],
      }),
    );
    expect(started).toHaveBeenCalled();
    noRawWords();
  });

  it('refuses a folder without git', () => {
    act(() =>
      root.render(
        <StartMissionSheet
          project={{ path: 'C:\\x', name: 'x', git: false }}
          entries={entries}
          activeMissions={0}
          api={fakeApi()}
          onClose={() => {}}
          onStarted={() => {}}
          initialChecks={DETECTED}
        />,
      ),
    );
    type('#ck-mission-goal', 'Make checkout faster');
    expect(document.body.textContent).toContain('Missions need a git project, because they work in a separate copy.');
  });
});
