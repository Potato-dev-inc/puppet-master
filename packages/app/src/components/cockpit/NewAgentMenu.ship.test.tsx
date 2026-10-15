// @vitest-environment jsdom
// The folder step of the "+" menu: project chip, Recent projects, the folder picker, "Set up Git here".
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AccountsApi } from '../../lib/accounts';
import type { GitInitPlan, RecentProject, ShipApi } from '../../lib/ship-api';
import { NewAgentMenu, type ProjectChoice } from './NewAgentMenu';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const accounts = { list: async () => ({ agents: [], accounts: [] }) } as unknown as AccountsApi;
const ENTRIES = [{ type: 'claude', label: 'Claude Code' }];

const SHOP: RecentProject = { path: 'D:\\code\\shop', name: 'shop', state: 'repo', last_used_ms: 3 };
const SITE: RecentProject = { path: 'D:\\code\\site', name: 'site', state: 'folder', last_used_ms: 2 };
const GONE: RecentProject = { path: 'D:\\code\\gone', name: 'gone', state: 'missing', last_used_ms: 1 };
const PLAN: GitInitPlan = {
  path: SITE.path,
  name: 'site',
  state: 'folder',
  commands: ['git init', 'git add -A', 'git commit -m "First snapshot"'],
  files: 12,
  too_big: false,
  gitignore: ['node_modules/', '.env'],
};

function ship(over: Partial<ShipApi> = {}): ShipApi {
  return {
    recent: vi.fn().mockResolvedValue([SHOP, SITE, GONE]),
    pickFolder: vi.fn().mockResolvedValue(null),
    forget: vi.fn().mockResolvedValue(undefined),
    gitInitPlan: vi.fn().mockResolvedValue(PLAN),
    gitInit: vi.fn().mockResolvedValue({ commit: 'abc1234', files: 12, wrote_gitignore: true }),
    ...over,
  } as unknown as ShipApi;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes(text));
const click = async (text: string) => {
  const el = button(text);
  if (!el) throw new Error(`no button "${text}" in: ${host.textContent}`);
  await act(async () => el.click());
};

async function mount(props: { project?: ProjectChoice | null; api?: ShipApi; isolated?: boolean } = {}) {
  const onProject = vi.fn();
  const onIsolated = vi.fn();
  const api = props.api ?? ship();
  await act(async () => {
    root.render(
      <NewAgentMenu
        entries={ENTRIES}
        readyAgents={new Set()}
        onStart={() => undefined}
        onClose={() => undefined}
        accountsApi={accounts}
        project={props.project === undefined ? { path: SHOP.path, name: 'shop', state: 'repo' } : props.project}
        onProject={onProject}
        isolated={props.isolated ?? false}
        onIsolated={onIsolated}
        ship={api}
      />,
    );
  });
  return { onProject, onIsolated, api };
}

describe('project chip', () => {
  it('shows the project at the top of the menu', async () => {
    await mount();
    const chip = host.querySelector('.ck-chip');
    expect(chip?.textContent).toContain('shop');
    expect(host.querySelector('.ck-menu-cap')?.textContent).toContain('New agent');
  });

  it('is not drawn by the plain menu (no onProject)', async () => {
    await act(async () => {
      root.render(<NewAgentMenu entries={ENTRIES} readyAgents={new Set()} onStart={() => undefined} onClose={() => undefined} accountsApi={accounts} />);
    });
    expect(host.querySelector('.ck-chip')).toBeNull();
  });
});

describe('Recent projects', () => {
  it('lists the recent folders with their state and picks a Git project', async () => {
    const { onProject } = await mount();
    await click('shop');
    expect(host.textContent).toContain('Recent projects');
    expect(host.textContent).toContain('site');
    expect(host.textContent).toContain('No Git yet');
    expect(host.textContent).toContain('Folder not found');
    const rows = Array.from(host.querySelectorAll('.ck-menu-item--project'));
    await act(async () => (rows[0] as HTMLButtonElement).click());
    expect(onProject).toHaveBeenCalledWith({ path: SHOP.path, name: 'shop', state: 'repo' });
    expect(host.textContent).toContain('New agent');
  });

  it('greys out a deleted folder and lets you remove it from the list', async () => {
    const api = ship();
    await mount({ api });
    await click('shop');
    const row = Array.from(host.querySelectorAll<HTMLButtonElement>('.ck-menu-item--project')).find((b) => b.textContent?.includes('gone'));
    expect(row?.disabled).toBe(true);
    await click('Remove');
    expect(api.forget).toHaveBeenCalledWith(GONE.path);
  });

  it('says so when there is nothing yet', async () => {
    await mount({ api: ship({ recent: vi.fn().mockResolvedValue([]) }) });
    await click('shop');
    expect(host.textContent).toContain('No projects yet');
  });

  it('opens the native folder picker and goes on with the chosen folder', async () => {
    const api = ship({ pickFolder: vi.fn().mockResolvedValue({ ...SHOP, path: 'D:\\code\\new', name: 'new' }) });
    const { onProject } = await mount({ api });
    await click('shop');
    await click('Choose folder');
    expect(api.pickFolder).toHaveBeenCalled();
    expect(onProject).toHaveBeenCalledWith({ path: 'D:\\code\\new', name: 'new', state: 'repo' });
  });

  it('stays put when the picker is cancelled', async () => {
    const { onProject } = await mount();
    await click('shop');
    await click('Choose folder');
    expect(onProject).not.toHaveBeenCalled();
    expect(host.textContent).toContain('Recent projects');
  });
});

describe('Set up Git here', () => {
  async function toGitStep(api = ship()) {
    const rendered = await mount({ api });
    await click('shop');
    const row = Array.from(host.querySelectorAll<HTMLButtonElement>('.ck-menu-item--project')).find((b) => b.textContent?.includes('site'));
    await act(async () => row?.click());
    return { ...rendered, api };
  }

  it('shows what will run BEFORE anything runs', async () => {
    const { api } = await toGitStep();
    expect(host.textContent).toContain("This folder doesn't use Git yet. Undo and separate copies need it.");
    expect(host.textContent).toContain('git init');
    expect(host.textContent).toContain('git commit -m "First snapshot"');
    expect(host.textContent).toContain('Saves 12 files as the first snapshot.');
    expect(host.textContent).toContain('Nothing is uploaded.');
    expect(host.textContent).toContain('.gitignore');
    expect(api.gitInitPlan).toHaveBeenCalledWith(SITE.path);
    expect(api.gitInit).not.toHaveBeenCalled();
  });

  it('keeps the commands under Details and puts the two choices in one row', async () => {
    await toGitStep();
    const details = host.querySelector('details.ck-gitstep-details') as HTMLDetailsElement;
    expect(details.open).toBe(false);
    expect(details.querySelector('summary')!.textContent).toBe('Details');
    expect(details.textContent).toContain('git init');
    expect(host.textContent).not.toContain('Use without undo');
    const row = host.querySelector('.ck-gitstep-actions')!;
    expect([...row.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['Set up Git here', 'Not now']);
  });

  it('runs it on the button and then uses the folder as a Git project', async () => {
    const { api, onProject } = await toGitStep();
    await click('Set up Git here');
    expect(api.gitInit).toHaveBeenCalledWith(SITE.path);
    expect(onProject).toHaveBeenCalledWith({ path: SITE.path, name: 'site', state: 'repo' });
  });

  it('can be skipped with "Not now"', async () => {
    const { api, onProject } = await toGitStep();
    await click('Not now');
    expect(api.gitInit).not.toHaveBeenCalled();
    expect(onProject).toHaveBeenCalledWith({ path: SITE.path, name: 'site', state: 'folder' });
  });

  it('refuses a folder that is too big and shows the error of a failed run', async () => {
    const big = ship({ gitInitPlan: vi.fn().mockResolvedValue({ ...PLAN, too_big: true, files: 20001 }) });
    await toGitStep(big);
    expect(host.textContent).toContain('too many files');
    expect(button('Set up Git here')?.disabled).toBe(true);
  });

  it('shows why setting up failed', async () => {
    const failing = ship({ gitInit: vi.fn().mockRejectedValue(new Error('GIT_INIT_FAILED: git commit failed: no identity')) });
    await toGitStep(failing);
    await click('Set up Git here');
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('git commit failed');
  });
});

describe('Separate copy switch', () => {
  it('works for a Git project and explains itself when on', async () => {
    const { onIsolated } = await mount({ isolated: true });
    const sw = host.querySelector<HTMLButtonElement>('[role="switch"]');
    expect(sw?.getAttribute('aria-checked')).toBe('true');
    expect(host.textContent).toContain("your uncommitted changes aren't included");
    await act(async () => sw?.click());
    expect(onIsolated).toHaveBeenCalledWith(false);
  });

  it('is off and says why for a folder without Git', async () => {
    await mount({ project: { path: SITE.path, name: 'site', state: 'folder' }, isolated: true });
    const sw = host.querySelector<HTMLButtonElement>('[role="switch"]');
    expect(sw?.disabled).toBe(true);
    expect(sw?.getAttribute('aria-checked')).toBe('false');
    expect(host.textContent).toContain('Needs Git');
  });
});
