// @vitest-environment jsdom
// Settings > Labs > Assign an issue: the switch, GitHub line, repositories, who may start a run, runs at once and
// the recent runs. `invoke` is mocked; every write must be one `issue_intake_save` or `issue_intake_act`.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FolderCheck, GhView, IntakeConfig, IntakeView, RepoStatusView, RepoWatch, RunView } from '../../../lib/issue-intake';
import { buttonByText, byLabel, click, settle, type as typeInto } from '../test-utils';
import { AssignIssueSettings, gitHubName, ghLine, prNumber, NAMES_CAP, RECENT_CAP } from './AssignIssueSettings';
import { REPOS_CAP } from './AddRepoSheet';

const invoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => null) }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  invoke.mockReset();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const repo = (over: Partial<RepoWatch> = {}): RepoWatch => ({
  id: 'rw-0a1b2c3d',
  owner_repo: 'acme/billing',
  folder: 'C:\\Users\\you\\code\\billing',
  label: 'puppet',
  agent: { agent_type: 'codex', account: null },
  turns: 30,
  minutes: 45,
  runs_per_day: 5,
  ask_before_start: true,
  ask_before_push: true,
  added_at_ms: 1_000,
  paused: false,
  ...over,
});

const row = (over: Partial<RepoWatch> = {}, status = 'Watching'): RepoStatusView => ({ repo: repo(over), agent_label: 'Codex', status });

const run = (over: Partial<RunView> = {}): RunView => ({
  id: 'ir-0123456789ab',
  repo: 'acme/billing',
  number: 42,
  title: 'Login button does nothing',
  url: 'https://github.com/acme/billing/issues/42',
  labeledBy: 'you',
  author: 'someone',
  state: 'running',
  stateText: '',
  agentLabel: 'Codex',
  prUrl: null,
  paneId: null,
  atMs: 1_000,
  forced: [],
  ...over,
});

const EMPTY_CONFIG: IntakeConfig = { enabled: false, allowed: [], write_access_counts: false, parallel: 1, repos: [], ignored: [] };
const READY: GhView = { state: 'ready', login: 'you' };

function view(over: Partial<IntakeView> = {}): IntakeView {
  return { config: EMPTY_CONFIG, gh: READY, repos: [], runs: [], ...over };
}

/** A fake backend: holds the view, applies saves, and answers acts and checks. */
function backend(start: IntakeView, check?: FolderCheck) {
  const state = { view: start };
  invoke.mockImplementation(async (cmd: string, args?: { config?: IntakeConfig }) => {
    switch (cmd) {
      case 'issue_intake_view':
        return state.view;
      case 'issue_intake_save':
        state.view = { ...state.view, config: args!.config! };
        return state.view;
      case 'issue_intake_act':
        return run({ state: 'queued' });
      case 'issue_intake_poll_now':
        return undefined;
      case 'issue_intake_check_folder':
        return check ?? { owner_repo: 'acme/billing', default_branch: 'main', gh: 'ready', why_not: null };
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  });
  return state;
}

async function mount(start: IntakeView) {
  backend(start);
  await act(async () => root.render(<AssignIssueSettings />));
  await settle(6);
}

const text = () => host.textContent ?? '';
const saves = (): IntakeConfig[] => invoke.mock.calls.filter((c) => c[0] === 'issue_intake_save').map((c) => (c[1] as { config: IntakeConfig }).config);
const commands = (name: string) => invoke.mock.calls.filter((c) => c[0] === name);
const switchFor = (label: string) => byLabel<HTMLButtonElement>(host, label);

describe('the main switch and GitHub line', () => {
  it('is off by default and says nothing is watched until a repository is added', async () => {
    await mount(view());
    expect(switchFor('Watch GitHub issues')?.getAttribute('aria-checked')).toBe('false');
    expect(text()).toContain('Nothing is watched until you add a repository below.');
  });

  it('turning it on saves the whole config with enabled set and nothing else changed', async () => {
    const start = view({ config: { ...EMPTY_CONFIG, parallel: 2, allowed: ['pat'] } });
    await mount(start);
    await click(switchFor('Watch GitHub issues'));
    await settle(3);
    expect(saves()).toEqual([{ ...start.config, enabled: true }]);
  });

  it('names the GitHub login when gh is ready', async () => {
    await mount(view());
    expect(text()).toContain('Signed in to GitHub as @you (gh)');
  });

  it.each([
    ['missing', { state: 'missing', login: null } as GhView],
    ['signed out', { state: 'signed_out', login: null } as GhView],
  ])('says what to install when gh is %s, and disables Check now', async (_name, gh) => {
    await mount(view({ gh }));
    expect(text()).toContain('Install the GitHub CLI and run gh auth login.');
    expect(buttonByText(host, 'Check now')?.disabled).toBe(true);
  });

  it('ghLine has no login text without a login', () => {
    expect(ghLine({ state: 'ready', login: null })).toBe('Signed in to GitHub (gh)');
  });
});

describe('repositories', () => {
  it('shows the plain line for each repository and its status', async () => {
    await mount(view({ config: { ...EMPTY_CONFIG, enabled: true, repos: [repo()] }, repos: [row({}, 'Watching')] }));
    expect(text()).toContain('acme/billing · label puppet · Codex · 30 turns · 45 min · 5 a day');
    expect(text()).toContain('Watching');
  });

  it('pausing one repository saves it with paused set', async () => {
    const start = view({ config: { ...EMPTY_CONFIG, repos: [repo()] }, repos: [row()] });
    await mount(start);
    await click(buttonByText(host, 'Pause'));
    await settle(3);
    expect(saves()[0].repos[0].paused).toBe(true);
    expect(saves()[0].repos[0].id).toBe('rw-0a1b2c3d');
  });

  it('Remove asks first, then saves the list without that repository', async () => {
    await mount(view({ config: { ...EMPTY_CONFIG, repos: [repo()] }, repos: [row()] }));
    await click(buttonByText(host, 'Remove'));
    expect(host.querySelector('.st-sheet h2')?.textContent).toBe('Stop watching acme/billing?');
    expect(saves()).toEqual([]);
    await click(host.querySelector<HTMLButtonElement>('.st-sheet .st-btn.is-primary'));
    await settle(3);
    expect(saves()[0].repos).toEqual([]);
  });

  it('stops adding at 20 repositories', async () => {
    const repos = Array.from({ length: REPOS_CAP }, (_, i) => repo({ id: `rw-${i.toString(16).padStart(8, '0')}`, owner_repo: `acme/r${i}` }));
    await mount(view({ config: { ...EMPTY_CONFIG, repos } }));
    expect(buttonByText(host, '+ Add a repository')?.disabled).toBe(true);
    expect(text()).toContain('You can watch up to 20 repositories.');
  });

  it('Add a repository opens the add sheet and saves nothing until it is filled in', async () => {
    await mount(view());
    await click(buttonByText(host, '+ Add a repository'));
    expect(host.querySelector('.st-sheet h2')?.textContent).toBe('Add a repository');
    expect(byLabel<HTMLInputElement>(host, 'Folder on this PC')?.value).toBe('');
    expect(saves()).toEqual([]);
  });
});

describe('who can start a run', () => {
  it('adds a GitHub name in lower case through a save', async () => {
    await mount(view());
    await click(buttonByText(host, '+ Add a GitHub name'));
    await typeInto(byLabel<HTMLInputElement>(host, 'GitHub name')!, '@Some-One');
    await click(buttonByText(host, 'Add'));
    await settle(3);
    expect(saves()[0].allowed).toEqual(['some-one']);
  });

  it('refuses a bad name, your own login and a duplicate, and saves nothing', async () => {
    await mount(view({ config: { ...EMPTY_CONFIG, allowed: ['pat'] } }));
    await click(buttonByText(host, '+ Add a GitHub name'));
    const input = byLabel<HTMLInputElement>(host, 'GitHub name')!;
    await typeInto(input, 'bad name!');
    await click(buttonByText(host, 'Add'));
    expect(text()).toContain('Letters, numbers and dashes only.');
    await typeInto(input, 'you');
    await click(buttonByText(host, 'Add'));
    expect(text()).toContain('That is you already.');
    await typeInto(input, 'pat');
    await click(buttonByText(host, 'Add'));
    expect(text()).toContain('That name is already on the list.');
    expect(saves()).toEqual([]);
  });

  it('removes a name through a save', async () => {
    const start = view({ config: { ...EMPTY_CONFIG, allowed: ['pat', 'lee'] } });
    await mount(start);
    expect(text()).toContain('@pat');
    await click(buttonByText(host, 'Remove'));
    await settle(3);
    expect(saves()[0].allowed).toEqual(['lee']);
  });

  it(`stops adding at ${NAMES_CAP} names`, async () => {
    const allowed = Array.from({ length: NAMES_CAP }, (_, i) => `user-${i}`);
    await mount(view({ config: { ...EMPTY_CONFIG, allowed } }));
    expect(buttonByText(host, '+ Add a GitHub name')?.disabled).toBe(true);
    expect(text()).toContain('You can add up to 50 names.');
  });

  it('the write-access switch is off by default and saves when turned on', async () => {
    const start = view();
    await mount(start);
    expect(switchFor('Anyone with write access to the repo')?.getAttribute('aria-checked')).toBe('false');
    await click(switchFor('Anyone with write access to the repo'));
    await settle(3);
    expect(saves()[0].write_access_counts).toBe(true);
  });

  it('shows how many runs can go at once, between 1 and 3', async () => {
    await mount(view({ config: { ...EMPTY_CONFIG, parallel: 1 } }));
    expect(byLabel<HTMLInputElement>(host, 'Runs at once')?.value).toBe('1');
  });
});

describe('recent runs', () => {
  it('says so when there are none', async () => {
    await mount(view());
    expect(text()).toContain('No runs yet.');
  });

  it('a declined run offers Ask again, which acts on it and reloads', async () => {
    const declined = run({ id: 'ir-aaaaaaaaaaaa', number: 44, state: 'declined', stateText: 'Not now' });
    await mount(view({ runs: [declined] }));
    expect(text()).toContain('#44');
    expect(text()).toContain('Not now');
    await click(buttonByText(host, 'Ask again'));
    await settle(3);
    expect(commands('issue_intake_act')[0]).toEqual(['issue_intake_act', { runId: 'ir-aaaaaaaaaaaa', action: 'ask_again' }]);
    expect(commands('issue_intake_view').length).toBeGreaterThan(1);
  });

  it('an ignored run offers Stop ignoring', async () => {
    await mount(view({ runs: [run({ state: 'ignored', stateText: 'Ignored' })] }));
    await click(buttonByText(host, 'Stop ignoring'));
    await settle(3);
    expect(commands('issue_intake_act')[0][1]).toEqual({ runId: 'ir-0123456789ab', action: 'unignore' });
  });

  it('a finished pull request shows as a draft PR link to GitHub', async () => {
    const prUrl = 'https://github.com/acme/billing/pull/57';
    await mount(view({ runs: [run({ state: 'pr_open', stateText: '', prUrl })] }));
    const link = [...host.querySelectorAll('a')].find((a) => a.textContent === 'Draft PR #57');
    expect(link?.getAttribute('href')).toBe(prUrl);
  });

  it('a skipped label says why and has no button', async () => {
    const why = "Skipped: labeled by @x, who isn't on your list";
    await mount(view({ runs: [run({ state: 'skipped', stateText: why, labeledBy: 'x' })] }));
    expect(text()).toContain(why);
    expect(buttonByText(host, 'Ask again')).toBeUndefined();
  });

  it('shows at most 20 runs', async () => {
    const runs = Array.from({ length: RECENT_CAP + 5 }, (_, i) => run({ id: `ir-${String(i).padStart(12, '0')}`, number: 100 + i, stateText: 'Working' }));
    await mount(view({ runs }));
    expect(host.querySelectorAll('.st-audit-row').length).toBe(RECENT_CAP);
  });
});

describe('copy', () => {
  it('never says "auto" and says it never merges only once, in the footer', async () => {
    await mount(view({ config: { ...EMPTY_CONFIG, enabled: true, repos: [repo()], allowed: ['pat'] }, repos: [row()], runs: [run({ state: 'declined', stateText: 'Not now' })] }));
    const copy = text();
    expect(copy).toContain('never merges');
    expect(copy.replace('never merges', '')).not.toMatch(/merge/i);
    expect(copy).not.toMatch(/\bauto/i);
    expect(copy).not.toMatch(/autopilot|bot\b|fix it for me/i);
  });
});

describe('when the desktop app is not there', () => {
  it('says the setting could not be read, and does not crash', async () => {
    invoke.mockRejectedValue(new Error('Tauri command unavailable'));
    await act(async () => root.render(<AssignIssueSettings />));
    await settle(4);
    expect(text()).toContain('Could not read this setting: Tauri command unavailable');
  });
});

describe('helpers', () => {
  it('gitHubName lower-cases, drops a leading @, and allows a bot name', () => {
    expect(gitHubName('@Pat-Lee')).toEqual({ name: 'pat-lee' });
    expect(gitHubName('ci-helper[bot]')).toEqual({ name: 'ci-helper[bot]' });
    expect(gitHubName('two words')).toHaveProperty('error');
    expect(gitHubName('')).toHaveProperty('error');
  });

  it('prNumber reads only github.com pull links', () => {
    expect(prNumber('https://github.com/acme/billing/pull/57')).toBe('57');
    expect(prNumber('https://example.com/pull/57')).toBeNull();
    expect(prNumber(null)).toBeNull();
  });

  it('the check result type is the one the sheet expects', () => {
    const check: FolderCheck = { owner_repo: 'acme/billing', default_branch: 'main', gh: 'ready', why_not: null };
    expect(check.owner_repo).toBe('acme/billing');
  });
});
