// @vitest-environment jsdom
// "Start crew…": pick a crew and a folder, start, and read the result. A failed role is named and the roles after it are listed as not started.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CrewStartOutcome, CrewSummary, CrewsApi } from '../../../lib/crews-api';
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

const crews = (): CrewSummary[] => [
  { slug: 'broken', name: 'Broken crew', roles: 2, agents: ['gone'], problem: 'The agent "gone" for Lead is not set up here.' },
  { slug: 'review-team', name: 'Review team', roles: 3, agents: ['alpha', 'wizard_made'], problem: null },
];

const ok: CrewStartOutcome = {
  started: [
    { role: 'Lead', agent_type: 'alpha', pane_id: 'p1' },
    { role: 'Checker', agent_type: 'wizard_made', pane_id: 'p2' },
  ],
  failed: null,
  not_started: [],
};

function api(over: Partial<CrewsApi> = {}): CrewsApi {
  return { list: vi.fn(async () => crews()), draft: vi.fn(), save: vi.fn(), remove: vi.fn(async () => {}), start: vi.fn(async () => ok), ...over } as CrewsApi;
}

const names: Record<string, string> = { alpha: 'Alpha', wizard_made: 'Wizard Made' };

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

describe('StartCrewSheet', () => {
  it('lists the crews with their agents, flags a broken one, and picks the first usable one', async () => {
    await render(api());
    const radios = [...host.querySelectorAll<HTMLElement>('[role="radio"]')];
    expect(radios).toHaveLength(2);
    expect(radios[0].textContent).toContain('is not set up here');
    expect(radios[1].textContent).toContain('3 agents · Alpha, Wizard Made');
    expect(radios[1].getAttribute('aria-checked')).toBe('true');
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
    expect(a.start).toHaveBeenCalledWith('review-team', 'C:\\work\\shop');
  });

  it('starts in the folder of the open pane by default and shows who started', async () => {
    const a = api();
    await render(a, { defaultFolder: 'C:\\work\\shop' });
    await act(async () => button('Start crew').click());
    expect(a.start).toHaveBeenCalledWith('review-team', 'C:\\work\\shop');
    const result = host.querySelector('[data-testid="crew-result"]')!;
    expect(result.textContent).toContain('Lead');
    expect(result.textContent).toContain('Checker');
    expect(host.textContent).toContain('Crew started');
  });

  it('a broken crew cannot be started', async () => {
    await render(api(), { defaultFolder: 'C:\\work\\shop' });
    act(() => host.querySelectorAll<HTMLElement>('[role="radio"]')[0].click());
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

  it('shows an empty state that says where to save a team', async () => {
    await render(api({ list: vi.fn(async () => []) }));
    expect(host.querySelector('[data-testid="crew-empty"]')?.textContent).toContain('Save this team');
    expect(button('Start crew').disabled).toBe(true);
  });

  it('removes a crew and reads the list again', async () => {
    const a = api();
    await render(a);
    await act(async () => button('Remove').click());
    expect(a.remove).toHaveBeenCalledWith('broken');
    expect(a.list).toHaveBeenCalledTimes(2);
  });
});
