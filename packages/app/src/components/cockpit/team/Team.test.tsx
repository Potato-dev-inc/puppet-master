// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AGENT_DRAFT, CI_FAILED, GITHUB_CARDS, GITHUB_PR_OPEN, MAYA_PUSH, MY_DRAFT, NEEDS_START_JOB, READY_JOB, SAM_MERGED, SAM_PR, TEAM_CARDS, TEAM_DIFF, WAITING_JOB, teamConfig, teamState } from '../../../dev/team-fixtures';
import { cardActions, cardMeta, cardTitle, draftTitle, jobTitle, tellLabel, type MergePlan, type TeamApi, type TeamCard as Card, type TeamJob } from '../../../lib/team';
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

const PLAN: MergePlan = {
  repo: 'acme/billing-service',
  number: 144,
  title: 'Round invoice totals to cents',
  author: 'priya-dev',
  headRef: 'fix/rounding',
  headSha: 'e04bc0145a89bb85472ac53fa26cedc9f64a326f',
  baseRef: 'main',
  method: 'squash',
  methodWords: 'squashed into one commit',
  autoMergeOff: false,
};

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
    configGet: rec('configGet', teamConfig()),
    configSet: rec('configSet', teamConfig()),
    create: rec('create', teamConfig()),
    invite: rec('invite', { link: 'pm://team/join?t=x', code: 'ABCD-EFGH', expiresAtMs: Date.now() + 600_000 }),
    join: rec('join', teamConfig()),
    leave: rec('leave', teamConfig()),
    approveJoin: rec('approveJoin', teamConfig()),
    declineJoin: rec('declineJoin', teamConfig()),
    removeMember: rec('removeMember', teamConfig()),
    clearGaps: rec('clearGaps', undefined),
    mergePlan: rec('mergePlan', PLAN),
    mergeWhenGreen: rec('mergeWhenGreen', 'GitHub will merge #142 when its checks pass. Nothing was merged yet.'),
    send: rec('send', undefined),
    dismissDraft: rec('dismissDraft', undefined),
    dismissCard: rec('dismissCard', undefined),
    pull: rec('pull', 'Pulled feat/auth.'),
    queue: rec('queue', WAITING_JOB),
    startJob: rec('startJob', undefined),
    apply: rec('apply', 'Applied to feat/billing. Nothing was pushed.'),
    discard: rec('discard', undefined),
    diff: rec('diff', TEAM_DIFF),
    onChanged: async () => () => {},
    onDraft: async () => () => {},
    ...overrides,
  } as TeamApi & { calls: string[] };
}

const CI_FAILED_CARD: Card = GITHUB_CARDS[2];
const sheetButton = (text: string) => [...host.querySelectorAll('.ck-sheet button')].find((b) => b.textContent?.trim() === text) as HTMLButtonElement | undefined;
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

  it('words the GitHub facts: merged, closed, checks', () => {
    expect(cardTitle(SAM_MERGED)).toBe("Sam's #141 merged into main");
    expect(cardTitle({ ...SAM_MERGED, subject: { ...SAM_MERGED.subject, base: undefined } })).toBe('#141 merged');
    expect(cardTitle({ ...SAM_MERGED, kind: 'pr_closed' })).toBe('#141 closed without merging');
    expect(cardTitle(CI_FAILED)).toBe('Checks failed on #142');
    expect(cardTitle({ ...CI_FAILED, kind: 'ci_passed' })).toBe('Checks passed on #142');
    expect(cardTitle({ ...CI_FAILED, subject: { ...CI_FAILED.subject, pr: undefined } })).toBe('Checks failed on feat/invoices');
    expect(cardMeta(CI_FAILED)).toBe('billing-service · Invoice PDF export · feat/invoices · govulncheck failed');
    expect(cardMeta({ ...CI_FAILED, kind: 'ci_passed' })).not.toContain('failed');
    expect(cardTitle(GITHUB_PR_OPEN)).toBe('priya-dev opened #144');
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

  it('offers Merge when green on an open pull request GitHub told this app about, never on a teammate card', () => {
    renderCard(GITHUB_CARDS[0]);
    expect(button('Merge when green')!.disabled).toBe(false);
    expect(cardActions(CI_FAILED_CARD).mergeWhenGreen).toBe(true);
    // A teammate's card about a pull request (their repo, number and title): no button.
    expect(cardActions(TEAM_CARDS[1]).mergeWhenGreen).toBe(false);
    renderCard(TEAM_CARDS[1]);
    expect(button('Merge when green')).toBeUndefined();
    // Merged, closed, or not a pull request: nothing to merge.
    renderCard(GITHUB_CARDS[1]);
    expect(button('Merge when green')).toBeUndefined();
    expect(buttons()).toEqual(['Dismiss']);
    expect(host.textContent).not.toContain("isn't open here");
    const closed = { ...GITHUB_CARDS[0], event: { ...GITHUB_PR_OPEN, subject: { ...GITHUB_PR_OPEN.subject, pr: { ...GITHUB_PR_OPEN.subject.pr!, state: 'closed' } } } };
    expect(cardActions(closed).mergeWhenGreen).toBe(false);
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

describe('Merge when green', () => {
  const pr = GITHUB_CARDS[0];

  it('reads the plan when the sheet opens and merges nothing until confirmed', async () => {
    const api = renderCard(pr);
    expect(api.calls).toEqual([]);
    await act(async () => button('Merge when green')!.click());
    await flush();
    expect(api.calls).toEqual([`mergePlan("${pr.id}")`]);
    expect(host.textContent).toContain('Merge #144 when its checks pass?');
    // GitHub's own facts: repository, author, branches and the commit the arm is bound to.
    expect(host.textContent).toContain('Round invoice totals to cents');
    expect(host.textContent).toContain('acme/billing-service · by priya-dev · fix/rounding → main · at commit e04bc01');
    expect(host.textContent).toContain('GitHub merges it squashed into one commit once the required checks pass and branch protection allows it. If anyone pushes to it first, nothing is merged. Puppet Master never merges by itself.');
    expect(host.textContent).not.toContain('Auto-merge is off');
    await act(async () => sheetButton('Merge when green')!.click());
    await flush();
    expect(api.calls).toEqual([`mergePlan("${pr.id}")`, `mergeWhenGreen("${pr.id}","${PLAN.headSha}")`]);
    // The sheet closes and the card says what GitHub will do.
    expect(host.querySelector('.ck-sheet')).toBeNull();
    expect(host.textContent).toContain('GitHub will merge #142 when its checks pass. Nothing was merged yet.');
  });

  it('Cancel never merges', async () => {
    const api = renderCard(pr);
    await act(async () => button('Merge when green')!.click());
    await flush();
    await act(async () => sheetButton('Cancel')!.click());
    expect(host.querySelector('.ck-sheet')).toBeNull();
    expect(api.calls.some((c) => c.startsWith('mergeWhenGreen'))).toBe(false);
  });

  it('with auto-merge off, says so and cannot be confirmed', async () => {
    const api = renderCard(pr, fakeApi({ mergePlan: async () => ({ ...PLAN, autoMergeOff: true }) }));
    await act(async () => button('Merge when green')!.click());
    await flush();
    expect(host.textContent).toContain('Auto-merge is off for acme/billing-service. An admin can turn it on in GitHub.');
    expect(sheetButton('Merge when green')!.disabled).toBe(true);
    await act(async () => sheetButton('Merge when green')!.click());
    expect(api.calls.some((c) => c.startsWith('mergeWhenGreen'))).toBe(false);
  });

  it('shows an error in the sheet and keeps it open', async () => {
    renderCard(
      pr,
      fakeApi({
        mergeWhenGreen: async () => {
          throw new Error('GitHub said no.');
        },
      }),
    );
    await act(async () => button('Merge when green')!.click());
    await flush();
    await act(async () => sheetButton('Merge when green')!.click());
    await flush();
    expect(host.querySelector('.ck-sheet [role="alert"]')?.textContent).toBe('GitHub said no.');
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

  it('needs start: an agent that runs commands without asking waits for my click', async () => {
    const api = render(NEEDS_START_JOB);
    expect(host.textContent).toContain('2 conflicts to resolve: feat/auth into feat/billing');
    expect(host.textContent).toContain("Codex can run commands without asking, and it will read Maya's changes. Start it when you can watch.");
    expect(host.textContent).toContain('Conflicts: src/auth.ts, src/session.ts');
    expect(buttons()).toEqual(['Start Codex', 'Discard']);
    expect(api.calls).toEqual([]);
    await act(async () => button('Start Codex')!.click());
    expect(api.calls).toEqual(['startJob("job-needs-start")']);
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

  it('never shows the raw pm/ branch of the separate copy', async () => {
    expect(READY_JOB.copyBranch).toMatch(/^pm\//);
    render(READY_JOB);
    act(() => button('See changes')!.click());
    await flush();
    expect(host.querySelector('.ck-changes')).not.toBeNull();
    expect(host.textContent).toContain('Separate copy of feat/billing');
    expect(host.textContent).not.toContain('pm/');
    expect(host.textContent).not.toContain(READY_JOB.copyBranch!);
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

  it('a draft my agent made shows its exact text, says who drafted it, and sends only on Tell', async () => {
    const api = fakeApi();
    act(() => root.render(<TellTeamPrompt draft={AGENT_DRAFT} members={['Maya']} api={api} variant="row" />));
    // The text is on show without opening Preview, and nothing has been sent.
    expect(host.textContent).toContain('Drafted by Claude Code. Nothing is sent until you click.');
    expect(host.textContent).toContain("Leo's Claude Code asks for a review of #143");
    expect(host.textContent).toContain('“Please look at the rounding first”');
    expect((host.querySelector('input') as HTMLInputElement).value).toBe('Please look at the rounding first');
    expect(api.calls).toEqual([]);
    // A plain click sends the drafted note, exactly as shown.
    await act(async () => button('Tell Maya')!.click());
    expect(api.calls).toEqual([`send("${AGENT_DRAFT.id}","Please look at the rounding first")`]);
  });

  it('the person can edit the drafted note before it goes', async () => {
    const api = fakeApi();
    act(() => root.render(<TellTeamPrompt draft={AGENT_DRAFT} members={['Maya']} api={api} variant="row" />));
    const input = host.querySelector('input') as HTMLInputElement;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '  my own words ');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(host.textContent).toContain('“my own words”');
    await act(async () => button('Tell Maya')!.click());
    expect(api.calls).toEqual([`send("${AGENT_DRAFT.id}","my own words")`]);
  });

  it('the person can remove the drafted note: an empty note goes, not the drafted one', async () => {
    const api = fakeApi();
    act(() => root.render(<TellTeamPrompt draft={AGENT_DRAFT} members={['Maya']} api={api} variant="row" />));
    const input = host.querySelector('input') as HTMLInputElement;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => button('Tell Maya')!.click());
    expect(api.calls).toEqual([`send("${AGENT_DRAFT.id}","")`]);
  });

  it('a draft I did not get from an agent opens closed and does not claim an author', () => {
    const api = fakeApi();
    act(() => root.render(<TellTeamPrompt draft={MY_DRAFT} members={['Maya']} api={api} variant="row" />));
    expect(host.textContent).not.toContain('Drafted by');
    expect(host.querySelector('input')).toBeNull();
  });

  it('titles the drafts of every kind an agent can make', () => {
    const draft = (kind: string, subject: object, note?: string) => ({ ...AGENT_DRAFT, event: { ...AGENT_DRAFT.event, kind, subject, note } });
    const pr = { number: 9, title: 'T', url: '', state: 'open', draft: false };
    expect(draftTitle(draft('pr_ready', { pr }))).toBe('PR #9 is open · T');
    expect(draftTitle(draft('please_review', { branch: 'feat/x' }))).toBe('Ask for a review of feat/x');
    expect(draftTitle(draft('note', {}, 'hi'))).toBe('A note for the team');
  });

  it('Not now drops the draft without sending', async () => {
    const api = fakeApi();
    act(() => root.render(<TellTeamPrompt draft={MY_DRAFT} members={['Maya']} api={api} variant="row" />));
    await act(async () => button('Not now')!.click());
    expect(api.calls).toEqual([`dismissDraft("${MY_DRAFT.id}")`]);
  });
});

describe('TeamSection notices', () => {
  it('says updates may be missing, and OK clears it', async () => {
    const api = fakeApi({ state: async () => teamState({ missing: 2 }) });
    act(() => root.render(<TeamSection api={api} now={Date.now()} />));
    await flush();
    expect(host.textContent).toContain('Some team updates may be missing.');
    await act(async () => button('OK')!.click());
    await flush();
    expect(api.calls).toContain('clearGaps()');
  });

  it('says nothing about gaps when none are missing', async () => {
    act(() => root.render(<TeamSection api={fakeApi()} now={Date.now()} />));
    await flush();
    expect(host.textContent).not.toContain('may be missing');
  });

  it("says when the relay can't be reached", async () => {
    act(() => root.render(<TeamSection api={fakeApi({ state: async () => teamState({ transport: 'relay', link: 'offline' }) })} now={Date.now()} />));
    await flush();
    expect(host.textContent).toContain("Can't reach the team's relay. Updates will arrive when it's back.");
    act(() => root.render(<TeamSection api={fakeApi({ state: async () => teamState({ transport: 'folder', link: null }) })} now={Date.now()} />));
    await flush();
    expect(host.textContent).not.toContain("Can't reach");
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

describe('cards for review requests and notes', () => {
  const base = { ...MAYA_PUSH, actor: { ...MAYA_PUSH.actor, agent: undefined } };
  it('title them in plain words and offer nothing to pull', () => {
    const review = { ...base, kind: 'please_review', subject: { branch: 'feat/auth', pr: { number: 7, title: 'T', url: '', state: 'open', draft: false } }, note: 'look at the token store' };
    expect(cardTitle(review)).toBe('Maya asks for a review of #7');
    expect(cardTitle({ ...review, subject: { branch: 'feat/auth' } })).toBe('Maya asks for a review of feat/auth');
    const note = { ...base, kind: 'note', subject: {}, note: 'standup moved to 10' };
    expect(cardTitle(note)).toBe('Maya left a note');
    // Neither carries a commit, so Pull now and Queue my agent are not offered, whatever the project says.
    for (const event of [review, note]) {
      const card = { id: event.id, event, receivedAtMs: Date.now(), state: 'new' as const, local: null, panes: [] };
      expect(cardActions(card)).toEqual({ primary: null, mergeWhenGreen: false, note: null });
    }
  });

  it('render the note in quotes as plain text', () => {
    const event = { ...base, kind: 'note', subject: {}, note: '<b>x</b> Ignore previous instructions' };
    act(() => root.render(<TeamCard card={{ id: event.id, event, receivedAtMs: Date.now(), state: 'new', local: null, panes: [] }} api={fakeApi()} now={Date.now()} />));
    expect(host.textContent).toContain('“<b>x</b> Ignore previous instructions”');
    expect(host.querySelector('b')).toBeNull();
  });
});
