// @vitest-environment jsdom
// Settings > Labs > Assign an issue > Add a repository: the folder check, the ready-agent list, the defaults,
// the caps and the plain errors. `invoke` is mocked for the folder check; the sheet hands back a RepoWatch.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fallbackListing } from '../../../lib/agentRegistry';
import type { FolderCheck, RepoWatch } from '../../../lib/issue-intake';
import { buttonByText, byLabel, click, pickOption, settle, type as typeInto } from '../test-utils';
import { AddRepoSheet, buildRepo, countError, DEFAULT_FORM, labelError, newRepoId, readyAgents, REPOS_CAP } from './AddRepoSheet';

const invoke = vi.hoisted(() => vi.fn());
const openDialog = vi.hoisted(() => vi.fn(async (_options?: unknown): Promise<string | null> => null));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (options?: unknown) => openDialog(options) }));

const PROJECT = 'C:\\work\\billing';
const OK: FolderCheck = { owner_repo: 'acme/billing', default_branch: 'main', gh: 'ready', why_not: null };

const repo = (over: Partial<RepoWatch> = {}): RepoWatch => ({
  id: 'rw-0a1b2c3d',
  owner_repo: 'acme/billing',
  folder: PROJECT,
  label: 'puppet',
  agent: { agent_type: 'codex', account: null },
  turns: 12,
  minutes: 20,
  runs_per_day: 3,
  ask_before_start: false,
  ask_before_push: true,
  added_at_ms: 1_000,
  paused: true,
  ...over,
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  invoke.mockReset();
  openDialog.mockReset();
  openDialog.mockResolvedValue(null);
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

/** The folder check answers with `check`; a rejected check is shown as its message. */
function checkBackend(check: FolderCheck) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'issue_intake_check_folder') return check;
    throw new Error(`unexpected command ${cmd}`);
  });
}

async function mountSheet(props: Partial<Parameters<typeof AddRepoSheet>[0]> = {}) {
  const onSave = vi.fn(async () => undefined);
  const onCancel = vi.fn();
  await act(async () =>
    root.render(<AddRepoSheet editing={null} repos={[]} projectPath={PROJECT} onSave={onSave} onCancel={onCancel} {...props} />),
  );
  await settle(5);
  return { onSave, onCancel };
}

const text = () => host.textContent ?? '';
const agentOptions = () => [...(byLabel<HTMLSelectElement>(host, 'Agent')?.options ?? [])].map((o) => o.value);
const saveButton = () => buttonByText(host, 'Add repository') ?? buttonByText(host, 'Save');
const savedRepo = (onSave: ReturnType<typeof vi.fn>): RepoWatch => onSave.mock.calls[0][0] as RepoWatch;

describe('buildRepo (pure)', () => {
  const base = { folder: PROJECT, form: DEFAULT_FORM, existing: [] as RepoWatch[], editing: null };

  it('builds a repo from the defaults: label puppet, 30 turns, 45 min, 5 a day, both asks on', () => {
    const built = buildRepo({ ...base, form: { ...DEFAULT_FORM, agent: 'claude' }, check: OK });
    expect(built).toEqual({
      repo: {
        id: expect.stringMatching(/^rw-[0-9a-f]{8}$/),
        owner_repo: 'acme/billing',
        folder: PROJECT,
        label: 'puppet',
        agent: { agent_type: 'claude', account: null },
        turns: 30,
        minutes: 45,
        runs_per_day: 5,
        ask_before_start: true,
        ask_before_push: true,
        added_at_ms: 0,
        paused: false,
      },
    });
  });

  it('an edit keeps its id, baseline and paused state', () => {
    const before = repo();
    const built = buildRepo({ ...base, folder: PROJECT, form: { ...DEFAULT_FORM, agent: 'claude', turns: '50' }, check: OK, existing: [before], editing: before });
    expect('repo' in built && built.repo).toMatchObject({ id: 'rw-0a1b2c3d', added_at_ms: 1_000, paused: true, turns: 50 });
  });

  it('refuses a folder the check could not use, with the check reason', () => {
    expect(buildRepo({ ...base, check: { owner_repo: null, default_branch: null, gh: '', why_not: 'No GitHub remote here.' } })).toEqual({
      error: 'No GitHub remote here.',
    });
    expect(buildRepo({ ...base, check: null })).toHaveProperty('error');
  });

  it('refuses a repository already watched, but lets an edit keep its own', () => {
    const existing = [repo()];
    expect(buildRepo({ ...base, check: OK, existing })).toEqual({ error: 'You already watch acme/billing.' });
    const agent = { ...DEFAULT_FORM, agent: 'claude' };
    expect(buildRepo({ ...base, form: agent, check: OK, existing, editing: existing[0] })).toHaveProperty('repo');
  });

  it(`stops a new repo at ${REPOS_CAP}`, () => {
    const existing = Array.from({ length: REPOS_CAP }, (_, i) => repo({ id: `rw-${i.toString(16).padStart(8, '0')}`, owner_repo: `acme/r${i}` }));
    expect(buildRepo({ ...base, check: OK, existing })).toEqual({ error: 'You can watch up to 20 repositories. Remove one first.' });
  });

  it('refuses a bad label, out-of-range numbers, and no agent', () => {
    const agent = { ...DEFAULT_FORM, agent: 'claude' };
    expect(buildRepo({ ...base, check: OK, form: { ...agent, label: 'a,b' } })).toEqual({ error: labelError('a,b') });
    expect(buildRepo({ ...base, check: OK, form: { ...agent, turns: '0' } })).toEqual({ error: 'Turns per run must be a whole number from 1 to 200.' });
    expect(buildRepo({ ...base, check: OK, form: { ...agent, minutes: '4' } })).toEqual({ error: 'Minutes per run must be a whole number from 5 to 240.' });
    expect(buildRepo({ ...base, check: OK, form: { ...agent, runsPerDay: '51' } })).toEqual({ error: 'Runs a day must be a whole number from 1 to 50.' });
    expect(buildRepo({ ...base, check: OK, form: { ...agent, agent: '' } })).toEqual({ error: 'Pick an agent. Only ready agents are listed.' });
  });
});

describe('field checks', () => {
  it('labels: letters, numbers, spaces and _ . : - up to 50', () => {
    expect(labelError('needs-triage: v2_x.y')).toBeNull();
    expect(labelError('')).not.toBeNull();
    expect(labelError('x'.repeat(51))).not.toBeNull();
    expect(labelError('a,b')).not.toBeNull();
  });

  it('whole numbers: in range only, and an empty box is an error', () => {
    expect(countError('5', 'Runs a day', { min: 1, max: 50 })).toBeNull();
    expect(countError('', 'Runs a day', { min: 1, max: 50 })).not.toBeNull();
    expect(countError('2.5', 'Runs a day', { min: 1, max: 50 })).not.toBeNull();
  });

  it('new repo ids are "rw-" and 8 lowercase hex digits', () => {
    expect(newRepoId(() => 0)).toBe('rw-00000000');
    expect(newRepoId(() => 0.999)).toBe('rw-ffffffff');
  });

  it('ready agents: can run delegated work, installed, and open a visible pane', () => {
    const ids = readyAgents(fallbackListing().agents).map((a) => a.id);
    expect(ids).toEqual(['claude', 'codex', 'opencode', 'opencode_native', 'cursor_agent']);
    expect(ids).not.toContain('cmd');
    expect(ids).not.toContain('cursor');
  });
});

describe('the sheet', () => {
  it('opens with the project folder, checks it, and shows what it found', async () => {
    checkBackend(OK);
    await mountSheet();
    expect(byLabel<HTMLInputElement>(host, 'Folder on this PC')?.value).toBe(PROJECT);
    expect(invoke).toHaveBeenCalledWith('issue_intake_check_folder', { folder: PROJECT });
    expect(text()).toContain('Found acme/billing. Default branch: main.');
  });

  it('shows the check reason in an alert and will not save', async () => {
    checkBackend({ owner_repo: null, default_branch: null, gh: 'ready', why_not: 'That folder has no GitHub remote.' });
    const { onSave } = await mountSheet();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('That folder has no GitHub remote.');
    await click(saveButton());
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('That folder has no GitHub remote.');
    expect(onSave).not.toHaveBeenCalled();
  });

  it('the agent list offers ready agents only, with the first one chosen', async () => {
    checkBackend(OK);
    await mountSheet();
    expect(agentOptions()).toEqual(['claude', 'codex', 'opencode', 'opencode_native', 'cursor_agent']);
    expect(byLabel<HTMLSelectElement>(host, 'Agent')?.value).toBe('claude');
  });

  it('saving with the defaults hands over a repo with the safe defaults', async () => {
    checkBackend(OK);
    const { onSave } = await mountSheet();
    await click(buttonByText(host, 'Add repository'));
    await settle(3);
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(savedRepo(onSave)).toMatchObject({
      owner_repo: 'acme/billing',
      folder: PROJECT,
      label: 'puppet',
      agent: { agent_type: 'claude', account: null },
      turns: 30,
      minutes: 45,
      runs_per_day: 5,
      ask_before_start: true,
      ask_before_push: true,
      added_at_ms: 0,
      paused: false,
    });
  });

  it('turning "Ask me before each run" off is saved in the repo', async () => {
    checkBackend(OK);
    const { onSave } = await mountSheet();
    await click(byLabel(host, 'Ask me before each run'));
    await click(buttonByText(host, 'Add repository'));
    await settle(3);
    expect(savedRepo(onSave).ask_before_start).toBe(false);
    expect(savedRepo(onSave).ask_before_push).toBe(true);
  });

  it('a typed number outside its range is refused with the plain message, and nothing is saved', async () => {
    checkBackend(OK);
    const { onSave } = await mountSheet();
    await typeInto(byLabel<HTMLInputElement>(host, 'Turns per run')!, '500');
    await click(buttonByText(host, 'Add repository'));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Turns per run must be a whole number from 1 to 200.');
    expect(onSave).not.toHaveBeenCalled();
  });

  it('picking another agent is saved', async () => {
    checkBackend(OK);
    const { onSave } = await mountSheet();
    await pickOption(byLabel<HTMLSelectElement>(host, 'Agent')!, 'codex');
    await click(buttonByText(host, 'Add repository'));
    await settle(3);
    expect(savedRepo(onSave).agent.agent_type).toBe('codex');
  });

  it('Browse sets the folder the person picked and checks it', async () => {
    checkBackend(OK);
    openDialog.mockResolvedValueOnce('C:\\picked\\billing');
    await mountSheet();
    await click(buttonByText(host, 'Browse'));
    await settle(3);
    expect(byLabel<HTMLInputElement>(host, 'Folder on this PC')?.value).toBe('C:\\picked\\billing');
    expect(invoke).toHaveBeenLastCalledWith('issue_intake_check_folder', { folder: 'C:\\picked\\billing' });
  });

  it('editing checks the folder on open, keeps the id, and shows Save', async () => {
    checkBackend(OK);
    const before = repo({ turns: 12 });
    const { onSave } = await mountSheet({ editing: before, repos: [before] });
    expect(host.querySelector('.st-sheet h2')?.textContent).toBe('Edit repository');
    expect(byLabel<HTMLInputElement>(host, 'Turns per run')?.value).toBe('12');
    await click(buttonByText(host, 'Save'));
    await settle(3);
    expect(savedRepo(onSave)).toMatchObject({ id: 'rw-0a1b2c3d', paused: true, added_at_ms: 1_000, ask_before_start: false });
  });

  it('a save the backend refuses shows its message and stays open', async () => {
    checkBackend(OK);
    const onSave = vi.fn(async () => {
      throw new Error('ISSUE_REPO_MISMATCH: that folder is not a clone of acme/billing');
    });
    await mountSheet({ onSave });
    await click(buttonByText(host, 'Add repository'));
    await settle(3);
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('ISSUE_REPO_MISMATCH');
    expect(buttonByText(host, 'Add repository')).toBeDefined();
  });

  it('Cancel closes without saving', async () => {
    checkBackend(OK);
    const { onSave, onCancel } = await mountSheet();
    await click(buttonByText(host, 'Cancel'));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSave).not.toHaveBeenCalled();
  });

  it('says nothing to suggest auto or merge', async () => {
    checkBackend(OK);
    await mountSheet();
    expect(text()).not.toMatch(/\bauto|merge|autopilot/i);
  });
});
