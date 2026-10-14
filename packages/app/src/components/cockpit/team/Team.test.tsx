// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MAYA_PUSH, MY_DRAFT, READY_JOB, SAM_PR, TEAM_CARDS, TEAM_DIFF, WAITING_JOB, teamState } from '../../../dev/team-fixtures';
import { cardActions, cardMeta, cardTitle, jobTitle, tellLabel, type TeamApi, type TeamCard as Card, type TeamJob } from '../../../lib/team';
import { TeamCard } from './TeamCard';
import { TeamJobCard } from './TeamJobCard';
import { TeamSection } from './TeamSection';
import { TellTeamPrompt } from './TellTeam';

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
});

function fakeApi(overrides: Partial<TeamApi> = {}): TeamApi & { calls: string[] } {
  const calls: string[] = [];
  const rec =
    <T,>(name: string, value: T) =>
    async (...args: unknown[]) => {
      calls.push(`${name}(${args.map((a) => JSON.stringify(a)).join(',')})`);
      return value;
    };
  return {
    calls,
    state: rec('state', teamState()),
    configGet: rec('configGet', { enabled: true, team: 'acme', folder: 'D:\\wire', name: 'Leo', members: ['Maya'], projects: [], profile: null }),
    configSet: rec('configSet', { enabled: true, team: 'acme', folder: 'D:\\wire', name: 'Leo', members: ['Maya'], projects: [], profile: null }),
    send: rec('send', undefined),
    dismissDraft: rec('dismissDraft', undefined),
    dismissCard: rec('dismissCard', undefined),
    pull: rec('pull', 'Pulled feat/auth.'),
    queue: rec('queue', WAITING_JOB),
    apply: rec('apply', 'Applied to feat/billing. Nothing was pushed.'),
    discard: rec('discard', undefined),
    diff: rec('diff', TEAM_DIFF),
    onChanged: async () => () => {},
    onDraft: async () => () => {},
    ...overrides,
  } as TeamApi & { calls: string[] };
}

const buttons = () => [...host.querySelectorAll('button')].map((b) => (b.textContent ?? '').trim());
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;
const flush = () => act(async () => {
  await new Promise((r) => setTimeout(r, 0));
});

function renderCard(card: Card, api = fakeApi()) {
  act(() => root.render(<ul><TeamCard card={card} api={api} now={MAYA_PUSH.atMs + 60_000} /></ul>));
  return api;
}

describe('copy', () => {
  it('names the person and their agent, the branch and the size', () => {
    expect(cardTitle(MAYA_PUSH)).toBe("Maya's Claude Code pushed feat/auth");
    expect(cardMeta(MAYA_PUSH)).toBe('billing-service · feat/auth · 3 files +84 −12');
    expect(cardTitle(SAM_PR)).toBe("Sam's Codex opened #142");
    expect(cardTitle({ ...MAYA_PUSH, actor: { ...MAYA_PUSH.actor, agent: undefined } })).toBe('Maya pushed feat/auth');
    expect(cardTitle({ ...MAYA_PUSH, subject: { ...MAYA_PUSH.subject, forced: true } })).toBe("Maya's Claude Code force-pushed feat/auth");
    expect(tellLabel(['Leo'])).toBe('Tell Leo');
    expect(tellLabel(['Leo', 'Sam'])).toBe('Tell Leo and Sam');
    expect(tellLabel([])).toBe('Tell the team');
    expect(jobTitle(READY_JOB)).toBe('feat/billing is up to date, in a separate copy');
  });
});

describe('TeamCard states', () => {
  const card = TEAM_CARDS[0];

  it('offers Queue my agent when I am on my own branch', () => {
    expect(cardActions(card).primary).toBe('queue');
    renderCard(card);
    expect(buttons()).toEqual(['Queue my agent…', 'Dismiss']);
  });

  it('offers Pull now when I am on the pushed branch with a clean tree', async () => {
    const onBranch = { ...card, local: { ...card.local!, currentBranch: 'feat/auth', canPull: true, canQueue: false } };
    const api = renderCard(onBranch);
    expect(buttons()).toEqual(['Pull now', 'Dismiss']);
    await act(async () => button('Pull now')!.click());
    expect(api.calls).toEqual([`pull("${card.id}")`]);
  });

  it('explains why nothing is offered (dirty tree, up to date, not cloned here)', () => {
    renderCard({ ...card, local: { ...card.local!, canQueue: false, canPull: false, dirty: true, note: 'You have uncommitted changes. Commit or stash them to pull.' } });
    expect(buttons()).toEqual(['Dismiss']);
    expect(host.textContent).toContain('You have uncommitted changes');
    renderCard({ ...card, local: null });
    expect(buttons()).toEqual(['Dismiss']);
    expect(host.textContent).toContain("isn't open here");
    renderCard({ ...card, state: 'queued' });
    expect(host.textContent).toContain('Queued for your agent.');
  });

  it('shows Merge when green on a GitHub PR, disabled as coming soon', () => {
    renderCard(TEAM_CARDS[1]);
    const merge = button('Merge when green')!;
    expect(merge.disabled).toBe(true);
    expect(merge.textContent).toContain('coming soon');
    renderCard(card);
    expect(button('Merge when green')).toBeUndefined();
  });

  it("renders a teammate's text as plain text and the note in quotes", () => {
    const hostile = {
      ...card,
      event: { ...card.event, note: '<img src=x onerror="alert(1)"> **bold**', actor: { ...card.event.actor, display: '<b>Mallory</b>' } },
    };
    renderCard(hostile);
    expect(host.querySelector('img')).toBeNull();
    expect(host.querySelector('b')).toBeNull();
    expect(host.textContent).toContain('“<img src=x onerror="alert(1)"> **bold**”');
    expect(host.textContent).toContain("<b>Mallory</b>'s Claude Code pushed feat/auth");
  });

  it('runs nothing until clicked, and Queue goes through the sheet', async () => {
    const api = renderCard(card);
    expect(api.calls).toEqual([]);
    act(() => button('Queue my agent')!.click());
    expect(host.textContent).toContain('Bring feat/auth into feat/billing');
    expect(host.textContent).toContain('After Claude Code finishes its current turn');
    expect(api.calls).toEqual([]);
    await act(async () => button('Queue it')!.click());
    expect(api.calls).toEqual([`queue("${card.id}","pane-claude-1","idle")`.replace('pane-claude-1', card.panes[0].paneId)]);
  });
});

describe('TeamJobCard states', () => {
  const render = (job: TeamJob, api = fakeApi()) => {
    act(() => root.render(<ul><TeamJobCard job={job} api={api} /></ul>));
    return api;
  };

  it('waiting: says when it starts; only Discard', () => {
    render(WAITING_JOB);
    expect(host.textContent).toContain('Starts when Claude Code finishes its current turn.');
    expect(buttons()).toEqual(['Discard']);
  });

  it('working: no buttons while the agent is in the copy', () => {
    render({ ...READY_JOB, state: 'working', message: null, stats: null });
    expect(host.textContent).toContain('Claude Code is merging feat/auth');
    expect(host.textContent).toContain('Resolving 2 conflicts in a separate copy.');
    expect(buttons()).toEqual([]);
  });

  it('ready: Apply / See changes / Discard; See changes opens the Changes view', async () => {
    const api = render(READY_JOB);
    expect(buttons()).toEqual(['Apply to my branch', 'See changes', 'Discard']);
    expect(host.textContent).toContain('Resolved: src/auth.ts, src/session.ts');
    act(() => button('See changes')!.click());
    await flush();
    expect(host.querySelector('.ck-changes')).not.toBeNull();
    expect(host.textContent).toContain('src/token.ts');
    // The Changes view is files only: Apply on the card is the one way in.
    expect(host.querySelector('.ck-changes-actions')).toBeNull();
    await act(async () => button('Apply to my branch')!.click());
    expect(api.calls).toContain('apply("job-ready")');
    expect(host.textContent).toContain('Nothing was pushed.');
  });

  it('failed: says why; Discard after a confirm', async () => {
    const api = render({ ...READY_JOB, state: 'failed', message: 'Your branch changed since. Redo the update?' });
    expect(host.textContent).toContain("Couldn't bring feat/auth into feat/billing");
    expect(host.textContent).toContain('Your branch changed since. Redo the update?');
    expect(buttons()).toEqual(['Discard']);
    act(() => button('Discard')!.click());
    expect(api.calls).toEqual([]);
    expect(host.textContent).toContain('Discard this update?');
    const confirm = [...host.querySelectorAll('.ck-sheet button')].find((b) => b.textContent === 'Discard') as HTMLButtonElement;
    await act(async () => confirm.click());
    expect(api.calls).toEqual(['discard("job-ready")']);
  });
});

describe('Tell the team?', () => {
  it('sends nothing until Tell is clicked, then sends with the note', async () => {
    const api = fakeApi();
    act(() => root.render(<TellTeamPrompt draft={MY_DRAFT} members={['Maya', 'Sam']} api={api} variant="toast" />));
    expect(host.textContent).toContain('Claude Code pushed feat/billing · 4 files +120 −9');
    expect(buttons()).toEqual(['Tell Maya and Sam', 'Not now', 'Preview']);
    act(() => button('Preview')!.click());
    expect(host.textContent).toContain("Leo's Claude Code pushed feat/billing");
    expect(host.textContent).toContain('No code is shared.');
    const input = host.querySelector('input') as HTMLInputElement;
    act(() => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
      setter.call(input, 'Careful with the session model');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(api.calls).toEqual([]);
    await act(async () => button('Tell Maya and Sam')!.click());
    expect(api.calls).toEqual([`send("${MY_DRAFT.id}","Careful with the session model")`]);
  });

  it('Not now drops the draft without sending', async () => {
    const api = fakeApi();
    act(() => root.render(<TellTeamPrompt draft={MY_DRAFT} members={['Maya']} api={api} variant="row" />));
    await act(async () => button('Not now')!.click());
    expect(api.calls).toEqual([`dismissDraft("${MY_DRAFT.id}")`]);
  });
});

describe('TeamSection', () => {
  it('renders nothing while Team is off', async () => {
    act(() => root.render(<TeamSection api={fakeApi({ state: async () => teamState({ enabled: false }) })} now={Date.now()} />));
    await flush();
    expect(host.textContent).toBe('');
  });

  it('lists unsent drafts, updates and teammate cards', async () => {
    const api = fakeApi({ state: async () => teamState({ drafts: [MY_DRAFT], jobs: [READY_JOB] }) });
    act(() => root.render(<TeamSection api={api} now={Date.now()} />));
    await flush();
    expect(host.querySelector('[aria-label="Unsent"]')).not.toBeNull();
    expect(host.querySelector('[aria-label="Updates for your branches"]')).not.toBeNull();
    expect(host.querySelectorAll('[data-team-card]').length).toBe(2);
    expect(vi.isMockFunction(api.send)).toBe(false);
  });
});
