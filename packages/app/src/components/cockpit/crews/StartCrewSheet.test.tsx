// @vitest-environment jsdom
// "Start crew…": pick a crew (mine or a starter) and a folder, check each role's agent, start, and read the result. A failed
// role is named and the roles after it are listed as not started. A starter is copied to My crews, never saved on its own.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Crew, CrewAgents, CrewStartOutcome, CrewSummary, CrewsApi } from '../../../lib/crews-api';
import { StartCrewSheet } from './StartCrewSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const mine = (): CrewSummary[] => [
  { slug: 'broken', name: 'Broken crew', roles: 2, agents: ['gone'], problem: 'The agent "gone" for Lead is not set up here.' },
  { slug: 'review-team', name: 'Review team', roles: 3, agents: ['alpha', 'wizard_made'], problem: null },
];

const built = (): CrewSummary[] => [
  { slug: 'starter:fix-the-tests', name: 'Fix the tests', roles: 2, agents: ['{default}'], problem: null },
  { slug: 'starter:research-write', name: 'Research + Write', roles: 2, agents: ['{default}'], problem: null },
];

const reviewTeam: Crew = {
  name: 'Review team',
  roles: [
    { name: 'Lead', agent_type: 'alpha', task: 'Plan {folder}' },
    { name: 'Checker', agent_type: 'wizard_made', task: 'Check {folder}', parent: 'Lead' },
  ],
};

const fixTests: Crew = {
  name: 'Fix the tests',
  roles: [
    { name: 'Fixer', agent_type: '{default}', task: 'Fix {folder}' },
    { name: 'Summarizer', agent_type: '{default}', task: 'Sum {folder}', parent: 'Fixer' },
  ],
};

const choices = (over: Partial<CrewAgents> = {}): CrewAgents => ({
  agents: [
    { id: 'alpha', label: 'Alpha' },
    { id: 'beta', label: 'Beta' },
    { id: 'wizard_made', label: 'Wizard Made' },
  ],
  default: 'alpha',
  ...over,
});

const ok: CrewStartOutcome = {
  started: [
    { role: 'Lead', agent_type: 'alpha', pane_id: 'p1' },
    { role: 'Checker', agent_type: 'wizard_made', pane_id: 'p2' },
  ],
  failed: null,
  not_started: [],
};

function api(over: Partial<CrewsApi> = {}): CrewsApi {
  const crews: Record<string, Crew> = { 'review-team': reviewTeam, 'starter:fix-the-tests': fixTests };
  return {
    list: vi.fn(async () => mine()),
    starters: vi.fn(async () => built()),
    get: vi.fn(async (slug: string) => crews[slug] ?? { name: slug, roles: [{ name: 'Lead', agent_type: 'alpha', task: 'x' }] }),
    agents: vi.fn(async () => choices()),
    draft: vi.fn(),
    save: vi.fn(async () => ({ slug: 'fix-the-tests', errors: [] })),
    remove: vi.fn(async () => {}),
    start: vi.fn(async () => ok),
    ...over,
  } as CrewsApi;
}

const names: Record<string, string> = { alpha: 'Alpha', beta: 'Beta', wizard_made: 'Wizard Made' };

async function render(a: CrewsApi, extra: { defaultFolder?: string | null; pick?: () => Promise<{ path: string } | null> } = {}) {
  const onClose = vi.fn();
  const ship = { pickFolder: vi.fn(extra.pick ?? (async () => null)) };
  await act(async () =>
    root.render(<StartCrewSheet api={a} ship={ship as never} defaultFolder={extra.defaultFolder ?? null} agentName={(id) => names[id] ?? id} onClose={onClose} />),
  );
  await act(async () => {});
  return { onClose, ship };
}

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;
const radio = (name: string) => [...host.querySelectorAll<HTMLElement>('[role="radio"]')].find((r) => r.textContent?.includes(name))!;
const select = (role: string) => host.querySelector<HTMLSelectElement>(`select[aria-label="Agent for ${role}"]`)!;

describe('StartCrewSheet', () => {
  it('lists my crews and the starters in their own sections, flags a broken one, and picks the first usable one', async () => {
    await render(api());
    const mineGroup = host.querySelector<HTMLElement>('[aria-label="My crews"]')!;
    const starterGroup = host.querySelector<HTMLElement>('[aria-label="Starters"]')!;
    expect(mineGroup.querySelectorAll('[role="radio"]')).toHaveLength(2);
    expect(starterGroup.querySelectorAll('[role="radio"]')).toHaveLength(2);
    expect(mineGroup.textContent).toContain('is not set up here');
    expect(radio('Review team').textContent).toContain('3 agents · Alpha, Wizard Made');
    expect(radio('Review team').getAttribute('aria-checked')).toBe('true');
    expect(radio('Fix the tests').textContent).toContain('2 agents · Default agent');
  });

  it('shows each role with the agent it will use, and a change of agent reaches the start call', async () => {
    const a = api();
    await render(a, { defaultFolder: 'C:\\work\\shop' });
    expect(select('Lead').value).toBe('alpha');
    expect(select('Checker').value).toBe('wizard_made');
    await act(async () => {
      select('Checker').value = 'beta';
      select('Checker').dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => button('Start crew').click());
    expect(a.start).toHaveBeenCalledWith('review-team', 'C:\\work\\shop', { Lead: 'alpha', Checker: 'beta' });
  });

  it('a starter role starts with the default agent, and the person can change it before starting', async () => {
    const a = api();
    await render(a, { defaultFolder: 'C:\\work\\shop' });
    await act(async () => radio('Fix the tests').click());
    expect(select('Fixer').value).toBe('alpha');
    expect(select('Summarizer').value).toBe('alpha');
    await act(async () => {
      select('Fixer').value = 'beta';
      select('Fixer').dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => button('Start crew').click());
    expect(a.start).toHaveBeenLastCalledWith('starter:fix-the-tests', 'C:\\work\\shop', { Fixer: 'beta', Summarizer: 'alpha' });
  });

  it('a starter whose default is not set up cannot start until a role gets an agent', async () => {
    const a = api({ agents: vi.fn(async () => choices({ default: null })) });
    await render(a, { defaultFolder: 'C:\\work\\shop' });
    await act(async () => radio('Fix the tests').click());
    expect(select('Fixer').value).toBe('');
    expect(host.textContent).toContain('No agent is set up on this computer for the default role.');
    expect(button('Start crew').disabled).toBe(true);
    await act(async () => {
      select('Fixer').value = 'beta';
      select('Fixer').dispatchEvent(new Event('change', { bubbles: true }));
      select('Summarizer').value = 'beta';
      select('Summarizer').dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(button('Start crew').disabled).toBe(false);
  });

  it('needs a folder before it can start, and the folder picker supplies one', async () => {
    const a = api();
    const { ship } = await render(a, { pick: async () => ({ path: 'C:\\work\\shop' }) });
    expect(button('Start crew').disabled).toBe(true);
    await act(async () => button('Choose folder…').click());
    expect(ship.pickFolder).toHaveBeenCalled();
    expect(host.textContent).toContain('shop');
    expect(button('Start crew').disabled).toBe(false);
    await act(async () => button('Start crew').click());
    expect(a.start).toHaveBeenCalledWith('review-team', 'C:\\work\\shop', { Lead: 'alpha', Checker: 'wizard_made' });
  });

  it('starts in the folder of the open pane by default and shows who started', async () => {
    const a = api();
    await render(a, { defaultFolder: 'C:\\work\\shop' });
    await act(async () => button('Start crew').click());
    expect(a.start).toHaveBeenCalledWith('review-team', 'C:\\work\\shop', { Lead: 'alpha', Checker: 'wizard_made' });
    const result = host.querySelector('[data-testid="crew-result"]')!;
    expect(result.textContent).toContain('Lead');
    expect(result.textContent).toContain('Checker');
    expect(host.textContent).toContain('Crew started');
  });

  it('a broken crew cannot be started', async () => {
    await render(api(), { defaultFolder: 'C:\\work\\shop' });
    await act(async () => radio('Broken crew').click());
    expect(button('Start crew').disabled).toBe(true);
  });

  it('names the role that failed and lists the ones that waited', async () => {
    const failed: CrewStartOutcome = {
      started: [{ role: 'Lead', agent_type: 'alpha', pane_id: 'p1' }],
      failed: { role: 'Checker', code: 'AGENT_NOT_INSTALLED', message: 'raw' },
      not_started: ['Reporter'],
    };
    await render(api({ start: vi.fn(async () => failed) }), { defaultFolder: 'C:\\work\\shop' });
    await act(async () => button('Start crew').click());
    expect(host.textContent).toContain('The crew did not finish starting');
    expect(host.textContent).toContain("Checker isn't installed.");
    const rows = [...host.querySelectorAll('[data-testid="crew-result"] li')].map((li) => li.textContent);
    expect(rows).toEqual(['StartedLeadAlpha', 'FailedChecker', 'Not startedReporter']);
  });

  it('shows an empty note for my crews, and the starters are still there to start or copy', async () => {
    await render(api({ list: vi.fn(async () => []) }), { defaultFolder: 'C:\\work\\shop' });
    expect(host.querySelector('[data-testid="crew-empty"]')?.textContent).toContain('Save this team');
    expect(radio('Fix the tests')).toBeTruthy();
    await act(async () => radio('Fix the tests').click());
    expect(button('Start crew').disabled).toBe(false);
  });

  it('copies a starter into an editor that saves it to My crews only when the person saves', async () => {
    const a = api();
    await render(a, { defaultFolder: 'C:\\work\\shop' });
    await act(async () => button('Copy to My crews').click());
    expect(a.get).toHaveBeenCalledWith('starter:fix-the-tests');
    expect(a.save).not.toHaveBeenCalled();
    expect(host.textContent).toContain('Copy this crew');
    // The default agent is written out as the agent it stands for, so the copy is a plain team.
    expect(host.textContent).toContain('Alpha');
    await act(async () => host.querySelector<HTMLButtonElement>('.ck-sheet-actions .ck-btn--primary')!.click());
    expect(a.save).toHaveBeenCalledWith(
      { name: 'Fix the tests', roles: [{ ...fixTests.roles[0], agent_type: 'alpha' }, { ...fixTests.roles[1], agent_type: 'alpha' }] },
      false,
    );
    expect(host.textContent).toContain('Copied to My crews as Fix the tests');
    expect(a.list).toHaveBeenCalledTimes(2);
  });

  it('removes a crew and reads the list again', async () => {
    const a = api();
    await render(a);
    await act(async () => button('Remove').click());
    expect(a.remove).toHaveBeenCalledWith('broken');
    expect(a.list).toHaveBeenCalledTimes(2);
  });
});
