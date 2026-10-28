// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AHEAD_TITLE, type ScoreRate, type ScoreRow, type ScoresApi, type ScoresReport } from '../../lib/agent-scores';
import { AgentScores } from './AgentScores';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const PROJECT = { path: 'C:\\Users\\you\\shop', name: 'shop' };

function rate(count: number, total: number, sample: ScoreRate['sample']): ScoreRate {
  const low = total === 0 ? 0 : Math.max(0, count / total - 0.2);
  const high = total === 0 ? 1 : Math.min(1, count / total + 0.2);
  return { count, total, sample, low, high };
}

const NONE = rate(0, 0, 'none');

function row(over: Partial<ScoreRow>): ScoreRow {
  return {
    agentId: 'alpha',
    agentLabel: 'Alpha Code',
    accountId: null,
    accountLabel: 'Default account',
    turns: 0,
    enough: true,
    endedOk: NONE,
    checksGreen: NONE,
    merged: NONE,
    undone: NONE,
    askedYou: NONE,
    perMerged: null,
    waitingReview: 0,
    brokeTests: 0,
    turnMsMedian: null,
    linesMedian: null,
    ahead: [],
    ...over,
  };
}

const ALPHA = row({
  turns: 10,
  endedOk: rate(10, 10, 'steady'),
  checksGreen: rate(5, 5, 'early'),
  merged: rate(8, 10, 'steady'),
  undone: rate(1, 10, 'steady'),
  askedYou: rate(3, 10, 'steady'),
  perMerged: { runs: 4, turnsMedian: 2, busyMsMedian: 14 * 60_000 },
  waitingReview: 2,
  turnMsMedian: 4 * 60_000,
  linesMedian: 38,
  ahead: ['endedOk'],
});

const NIMBUS = row({
  agentId: 'nimbus',
  agentLabel: 'Nimbus',
  accountLabel: 'Work',
  accountId: 'acct-work',
  turns: 3,
  enough: false,
});

const FULL: ScoresReport = {
  scope: 'project',
  project: { id: 'a1b2c3d4e5f6', label: 'shop', turns: 13 },
  projects: [
    { id: 'a1b2c3d4e5f6', label: 'shop', turns: 13 },
    { id: '0f0f0f0f0f0f', label: 'notes', turns: 2 },
  ],
  days: 90,
  sinceMs: 0,
  minSample: 5,
  rows: [ALPHA, NIMBUS],
  notes: ['Counting Undo from now on.', 'Tokens are not counted. Only usage an agent reports itself could show here, and it is not saved per turn.'],
};

const EMPTY: ScoresReport = {
  scope: 'all',
  project: null,
  projects: [],
  days: null,
  sinceMs: 0,
  minSample: 5,
  rows: [],
  notes: [],
};

function fakeApi(report: ScoresReport | ((project?: string, days?: number | null) => ScoresReport)): ScoresApi & {
  report: ReturnType<typeof vi.fn>;
} {
  return {
    report: vi.fn(async (project?: string, days?: number | null) => (typeof report === 'function' ? report(project, days) : report)),
  };
}

async function mount(api: ScoresApi, project: { path: string; name: string } | null = PROJECT) {
  await act(async () => {
    root.render(<AgentScores api={api} project={project} onClose={() => undefined} />);
  });
  await act(async () => {});
}

const text = () => host.textContent ?? '';
const buttonWith = (label: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === label) as HTMLButtonElement;
const select = (label: string) => host.querySelector(`select[aria-label="${label}"]`) as HTMLSelectElement;
const cells = (rowIndex: number) => [...host.querySelectorAll('tbody tr')[rowIndex].querySelectorAll('td')];

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('AgentScores', () => {
  it('loads once for the selected pane project, with the 90 day window', async () => {
    const api = fakeApi(FULL);
    await mount(api);
    expect(api.report).toHaveBeenCalledTimes(1);
    expect(api.report).toHaveBeenLastCalledWith(PROJECT.path, 90);
    expect(host.querySelector('h1')?.textContent).toBe('Scores on your code');
    expect(buttonWith('This project: shop').getAttribute('aria-pressed')).toBe('true');
    expect(select('Window').value).toBe('90');
  });

  it('opens on All projects when no pane is selected', async () => {
    const api = fakeApi(EMPTY);
    await mount(api, null);
    expect(api.report).toHaveBeenLastCalledWith(undefined, 90);
    expect(buttonWith('This project').disabled).toBe(true);
    expect(buttonWith('All projects').getAttribute('aria-pressed')).toBe('true');
  });

  it('switches scope and asks again for all projects, then back to this project', async () => {
    const api = fakeApi(FULL);
    await mount(api);
    await act(async () => buttonWith('All projects').click());
    await act(async () => {});
    expect(api.report).toHaveBeenCalledTimes(2);
    expect(api.report).toHaveBeenLastCalledWith(undefined, 90);
    await act(async () => buttonWith('This project: shop').click());
    await act(async () => {});
    expect(api.report).toHaveBeenLastCalledWith(PROJECT.path, 90);
  });

  it('asks again with the new window for 30 days, 90 days and all time', async () => {
    const api = fakeApi(FULL);
    await mount(api);
    await act(async () => {
      select('Window').value = '30';
      select('Window').dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {});
    expect(api.report).toHaveBeenLastCalledWith(PROJECT.path, 30);
    await act(async () => {
      select('Window').value = 'all';
      select('Window').dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {});
    expect(api.report).toHaveBeenLastCalledWith(PROJECT.path, null);
    expect(select('Window').value).toBe('all');
  });

  it('asks for the project picked in the project list', async () => {
    const api = fakeApi(FULL);
    await mount(api);
    await act(async () => {
      select('Project').value = '0f0f0f0f0f0f';
      select('Project').dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {});
    expect(api.report).toHaveBeenLastCalledWith('0f0f0f0f0f0f', 90);
  });

  it('shows "8 of 10" with the steady percent, and the early tag on a small sample', async () => {
    await mount(fakeApi(FULL));
    const first = cells(0);
    expect(first[0].textContent).toBe('10');
    expect(first[1].textContent).toContain('10 of 10');
    expect(first[1].querySelector('.ck-scores-tag')).toBeNull();
    expect(first[3].textContent).toContain('8 of 10');
    expect(first[3].querySelector('.ck-scores-pct')?.textContent).toBe('80%');
    const early = cells(0)[2];
    expect(early.classList.contains('is-early')).toBe(true);
    expect(early.querySelector('.ck-scores-tag')?.textContent).toBe('early');
    expect(early.textContent).toContain('5 of 5');
    expect(early.querySelector('.ck-scores-pct')).toBeNull();
  });

  it('shows the ahead chip with its title only for the metrics that are ahead', async () => {
    await mount(fakeApi(FULL));
    const chips = host.querySelectorAll('.ck-scores-ahead');
    expect(chips).toHaveLength(1);
    expect(chips[0].textContent).toBe('ahead');
    expect(chips[0].getAttribute('title')).toBe(AHEAD_TITLE);
    expect(cells(0)[1].querySelector('.ck-scores-ahead')).not.toBeNull();
  });

  it('never puts an ahead chip on Asked you, even when it is in view', async () => {
    const forged = { ...ALPHA, ahead: ['askedYou' as never] };
    await mount(fakeApi({ ...FULL, rows: [forged] }));
    expect(host.querySelectorAll('.ck-scores-ahead')).toHaveLength(0);
    expect(cells(0)[6].querySelector('.ck-scores-ahead')).toBeNull();
  });

  it('shows a dash with the reason for a row with too few runs', async () => {
    const small = row({ agentId: 'tiny', agentLabel: 'Tiny', turns: 3, enough: true, endedOk: rate(2, 3, 'not_enough') });
    await mount(fakeApi({ ...FULL, rows: [small] }));
    const cell = cells(0)[1];
    expect(cell.textContent).toBe('-');
    expect(cell.getAttribute('title')).toBe('Only 3 so far. A number needs 5.');
  });

  it('writes one plain line for an agent with not enough runs', async () => {
    await mount(fakeApi(FULL));
    const notEnough = host.querySelector('tr.ck-scores-notenough');
    expect(notEnough?.textContent).toContain('Nimbus');
    expect(notEnough?.querySelector('td')?.getAttribute('colspan')).toBe('7');
    expect(notEnough?.textContent).toContain('Not enough runs yet. 3 turns so far.');
  });

  it('shows the typical turn line and the waiting count under the agent name', async () => {
    await mount(fakeApi(FULL));
    expect(host.querySelector('.ck-scores-detail')?.textContent).toBe('Typical turn 4 min · 38 lines · 2 waiting for review');
  });

  it('shows the broke-the-tests line in the agent cell only when the count is above 0', async () => {
    const broke = row({ agentId: 'beta', agentLabel: 'Beta', turns: 10, enough: true, endedOk: rate(9, 10, 'steady'), brokeTests: 2 });
    await mount(fakeApi({ ...FULL, rows: [ALPHA, broke] }));
    expect(host.querySelectorAll('.ck-scores-broke')).toHaveLength(1);
    const line = host.querySelector('.ck-scores-broke');
    expect(line?.textContent).toBe('Broke the tests 2 times');
    expect(line?.closest('th')?.textContent).toContain('Beta');
    expect(host.querySelectorAll('tbody tr')[0].querySelector('.ck-scores-broke')).toBeNull();
  });

  it('uses the singular for one broken turn', async () => {
    const once = row({ turns: 10, enough: true, endedOk: rate(9, 10, 'steady'), brokeTests: 1 });
    await mount(fakeApi({ ...FULL, rows: [once] }));
    expect(host.querySelector('.ck-scores-broke')?.textContent).toBe('Broke the tests 1 time');
  });

  it('does not change the ahead chips or the cells when the count is set', async () => {
    const plain = row({ turns: 10, enough: true, endedOk: rate(9, 10, 'steady'), ahead: ['endedOk'] });
    await mount(fakeApi({ ...FULL, rows: [plain] }));
    const before = host.querySelectorAll('.ck-scores-ahead').length;
    const cellsBefore = cells(0).map((td) => td.textContent);
    await mount(fakeApi({ ...FULL, rows: [{ ...plain, brokeTests: 3 }] }));
    expect(host.querySelectorAll('.ck-scores-ahead')).toHaveLength(before);
    expect(cells(0).map((td) => td.textContent)).toEqual(cellsBefore);
  });

  it('shows the per merged change, or a dash when it needs 3 merged runs', async () => {
    await mount(fakeApi(FULL));
    expect(cells(0)[5].textContent).toBe('2 turns · 14 min');
    const dash = cells(0)[5];
    expect(dash).toBeDefined();
    const noTypical = row({ agentId: 'beta', agentLabel: 'Beta', turns: 10, enough: true, endedOk: rate(9, 10, 'steady') });
    await act(async () => root.render(<AgentScores api={fakeApi({ ...FULL, rows: [noTypical] })} project={PROJECT} onClose={() => undefined} />));
    await act(async () => {});
    expect(cells(0)[5].textContent).toBe('-');
    expect(cells(0)[5].getAttribute('title')).toBe('Needs 3 merged runs.');
  });

  it('says what the columns mean, with a caption per header', async () => {
    await mount(fakeApi(FULL));
    const headers = [...host.querySelectorAll('thead th')];
    expect(headers.map((th) => th.childNodes[0]?.textContent)).toEqual([
      'Agent',
      'Turns',
      'Ended OK',
      'Checks green',
      'Merged',
      'Undone',
      'Per merged change',
      'Asked you',
    ]);
    const endedOk = host.querySelector('th[aria-describedby="ck-scores-cap-endedOk"]');
    expect(endedOk?.querySelector('#ck-scores-cap-endedOk')?.textContent).toBe('No crash or API error. This is reliability, not code quality.');
  });

  it('shows the empty state when there are no turns', async () => {
    await mount(fakeApi(EMPTY));
    expect(text()).toContain('No turns here yet. Run an agent in this project, then come back.');
    expect(host.querySelector('table')).toBeNull();
  });

  it('says only one agent has enough runs when that is the case', async () => {
    await mount(fakeApi(FULL));
    expect(text()).toContain('Only one agent has enough runs here, so nothing can be compared yet.');
    const two = { ...FULL, rows: [ALPHA, { ...ALPHA, agentId: 'gamma', agentLabel: 'Gamma' }] };
    await act(async () => root.render(<AgentScores api={fakeApi(two)} project={PROJECT} onClose={() => undefined} />));
    await act(async () => {});
    expect(text()).not.toContain('Only one agent has enough runs here');
  });

  it('shows a plain error with role alert when the report cannot be read', async () => {
    const api: ScoresApi = { report: vi.fn().mockRejectedValue(new Error('Open Puppet Master on your computer to see your scores.')) };
    await mount(api);
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('Open Puppet Master');
  });

  it('lists the notes from the backend, as plain sentences', async () => {
    await mount(fakeApi(FULL));
    const notes = [...host.querySelectorAll('.ck-scores-notes li')].map((li) => li.textContent);
    expect(notes).toEqual(FULL.notes);
  });

  it('has a How to read this section with the closing lines', async () => {
    await mount(fakeApi(FULL));
    const how = host.querySelector('.ck-scores-how');
    expect(how?.querySelector('summary')?.textContent).toBe('How to read this');
    expect(how?.textContent).toContain('Merged is your choice. It is not proof the code is right.');
    expect(how?.textContent).toContain('Agents never see this page.');
  });

  it('shows no folder path, pane id or chat text, and uses no colour class', async () => {
    await mount(fakeApi(FULL));
    expect(text()).not.toMatch(/[A-Z]:\\|pane-|\/Users\//);
    expect(text()).not.toContain(PROJECT.path);
    expect(host.innerHTML).not.toMatch(/class="[^"]*(?:red|green|blue|amber|orange|yellow|purple)/i);
  });
});
