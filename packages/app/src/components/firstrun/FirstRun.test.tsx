// @vitest-environment jsdom
// The welcome, step by step, with the backend faked: choices, skips, the folder guard, the first task and
// its receipt, and the pause for a sign-in window.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { AgentHealth, AgentHealthApi } from '../../lib/agent-health';
import type { RecentProject } from '../../lib/ship-api';
import { FirstRun, type FirstRunDeps } from './FirstRun';
import { T } from './copy';
import type { Seed } from './useFirstRun';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
  vi.useRealTimers();
});

const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); });
const button = (text: string, exact = true) =>
  Array.from(host.querySelectorAll('button')).find((b) => (exact ? b.textContent === text : b.textContent?.includes(text))) as HTMLButtonElement | undefined;
const tap = async (text: string, exact = true) => {
  const el = button(text, exact);
  expect(el, `a button "${text}"`).toBeTruthy();
  await act(async () => el!.click());
  await flush();
};
const text = () => host.textContent ?? '';

const SEED: Seed = { start: 'welcome', look: null, project: null, agentId: null };

const health = (over: Partial<AgentHealth> = {}): AgentHealth => ({
  id: 'claude',
  label: 'Claude Code',
  state: 'unchecked',
  installed: true,
  version: '2.1.4',
  reason: 'Installed. Press Test to check it can answer.',
  install_hint: null,
  login_hint: null,
  can_test: true,
  ...over,
});

const healthApi = (rows: AgentHealth[], tested: Partial<AgentHealth> = { state: 'ready', reason: 'Ready.' }): AgentHealthApi => ({
  check: vi.fn(async () => rows),
  test: vi.fn(async (id: string) => ({ ...rows.find((r) => r.id === id)!, ...tested })),
});

const recent = (over: Partial<RecentProject>): RecentProject => ({ path: 'D:\\code\\shop', name: 'shop', state: 'repo', last_used_ms: 1, ...over });

function deps(over: Partial<FirstRunDeps> = {}): FirstRunDeps & { patch: ReturnType<typeof vi.fn> } {
  const patch = vi.fn(async () => ({}) as never);
  return {
    first: {
      state: vi.fn(),
      startTask: vi.fn(async (project: string) => ({ project, handle: 'h1', pane_id: null, enforced: true })),
      taskStatus: vi.fn(async () => ({
        status: 'completed',
        result: 'It is a small shop.',
        error: null,
        receipt: { backend: 'claude', read_only: 'enforced', outcome: 'completed_unchecked', worktree: { state: 'discarded', files_changed: 0 } },
        pane_id: null,
      })),
    },
    ship: {
      recent: vi.fn(async () => [recent({})]),
      pickFolder: vi.fn(async () => null),
      forget: vi.fn(async () => undefined),
      gitInitPlan: vi.fn(async (path: string) => ({ path, name: 'x', state: 'folder' as const, commands: ['git init', 'git add -A'], files: 12, too_big: false, gitignore: [] })),
      gitInit: vi.fn(async () => ({ commit: 'abc', files: 12, wrote_gitignore: true })),
    },
    simple: { checkFolder: vi.fn(async () => ({ ok: true as const })) },
    patch,
    load: vi.fn(async () => ({}) as never),
    health: healthApi([health()]),
    ...over,
  } as never;
}

const cockpit = (over: Record<string, unknown> = {}) =>
  ({ notify: vi.fn(), newAgent: vi.fn(async () => undefined), select: vi.fn(), answer: vi.fn(), prompts: {}, ...over }) as unknown as CockpitApi;

async function mount(d: FirstRunDeps, seed: Seed = SEED, c: CockpitApi = cockpit(), onFinish = vi.fn()) {
  await act(async () => {
    root.render(<FirstRun c={c} seed={seed} onFinish={onFinish} deps={d} />);
  });
  await flush();
  return { c, onFinish };
}

describe('1. Welcome', () => {
  it('asks how you work and holds Continue until you answer', async () => {
    await mount(deps());
    expect(text()).toContain(T.title);
    expect(text()).toContain('How do you work?');
    expect(button('Continue')!.disabled).toBe(true);
    expect(host.querySelectorAll('[role="radio"]')).toHaveLength(3);
  });

  it('"I write code" keeps the normal cockpit and says where Simple mode is', async () => {
    const d = deps();
    await mount(d);
    await tap('I write code', false);
    expect(d.patch).toHaveBeenCalledWith({ simple_mode: false });
    expect(host.querySelector('[data-testid="look-note"]')!.textContent).toBe(T.noteCode);
    expect(button('Continue')!.disabled).toBe(false);
  });

  it.each(['I build with AI, not much code', "I don't code"])('"%s" turns Simple mode on and says how to switch it off', async (label) => {
    const d = deps();
    await mount(d);
    await tap(label, false);
    expect(d.patch).toHaveBeenCalledWith({ simple_mode: true });
    const note = host.querySelector('[data-testid="look-note"]')!.textContent!;
    expect(note).toContain('Simple mode is on');
    expect(note).toContain('Settings');
  });

  it('can be skipped at the first step, and finishes once', async () => {
    const d = deps();
    const { onFinish } = await mount(d);
    await tap(T.skipSetup);
    expect(onFinish).toHaveBeenCalledTimes(1);
  });

  it('writes nothing until something changes, then saves the step', async () => {
    const d = deps();
    await mount(d);
    expect(d.patch).not.toHaveBeenCalled();
    await tap('I write code', false);
    d.patch.mockClear();
    await tap('Continue');
    expect(d.patch).toHaveBeenCalledWith({ onboarding_step: 'agents' });
  });
});

describe('2. Your agents', () => {
  const at = (step: Seed['start']): Seed => ({ ...SEED, start: step, look: 'code' });

  it('shows the Set up agents list inline and needs one ready agent to go on', async () => {
    const d = deps();
    await mount(d, at('agents'));
    expect(host.querySelector('.ck-doctor--embedded')).not.toBeNull();
    expect(text()).toContain('Claude Code');
    expect(text()).toContain(T.agentsNone);
    expect(button('Continue')!.disabled).toBe(true);
    await tap('Test');
    expect(d.health!.test).toHaveBeenCalledWith('claude');
    expect(text()).toContain(T.agentsReady(1));
    expect(button('Continue')!.disabled).toBe(false);
  });

  it('can be skipped for now with no agent ready', async () => {
    const d = deps();
    await mount(d, at('agents'));
    await tap(T.skipStep);
    expect(text()).toContain(T.projectTitle);
  });

  it('Sign in opens the agent and pauses the welcome as a bar until you come back', async () => {
    const d = deps({ health: healthApi([health({ state: 'needs_login', reason: 'It needs you to sign in first.' })]) });
    const { c } = await mount(d, at('agents'));
    await tap('Sign in');
    expect(c.newAgent).toHaveBeenCalledWith('claude');
    expect(host.querySelector('.fr-bar')!.textContent).toContain(T.pausedBar);
    expect((host.querySelector('.fr-shell') as HTMLElement).hidden).toBe(true);
    await tap(T.pausedButton);
    expect(host.querySelector('.fr-bar')).toBeNull();
    expect((host.querySelector('.fr-shell') as HTMLElement).hidden).toBe(false);
  });
});

describe('3. Your first project', () => {
  const at: Seed = { ...SEED, start: 'project', look: 'code' };

  it('lists Recent, and a repository can be chosen', async () => {
    const d = deps();
    await mount(d, at);
    expect(text()).toContain('shop');
    expect(button('Continue')!.disabled).toBe(true);
    await tap('shop', false);
    expect(host.querySelector('[data-testid="picked-project"]')!.textContent).toContain(T.projectChosen);
    expect(button('Continue')!.disabled).toBe(false);
  });

  it('refuses a broad folder: the row says so, and a picked one is not accepted', async () => {
    const documents = recent({ path: 'C:\\Users\\Ana\\Documents', name: 'Documents', state: 'folder' });
    const d = deps({
      ship: { ...deps().ship, recent: vi.fn(async () => [documents]), pickFolder: vi.fn(async () => documents) },
      simple: { checkFolder: vi.fn(async () => ({ ok: false as const, code: 'FOLDER_TOO_BROAD', message: 'documents' })) },
    });
    await mount(d, at);
    const row = host.querySelector('.fr-row') as HTMLButtonElement;
    expect(row.disabled).toBe(true);
    await tap(T.chooseFolder);
    expect(host.querySelector('.fr-problem')!.textContent).toBe(T.tooBroad);
    expect(host.querySelector('[data-testid="picked-project"]')).toBeNull();
    expect(button('Continue')!.disabled).toBe(true);
    expect(d.simple.checkFolder).toHaveBeenCalledWith('C:\\Users\\Ana\\Documents');
  });

  it('a folder without Git shows the commands first; Set up Git here makes it ready', async () => {
    const plain = recent({ path: 'D:\\code\\page', name: 'page', state: 'folder' });
    const d = deps({ ship: { ...deps().ship, recent: vi.fn(async () => [plain]), gitInitPlan: deps().ship.gitInitPlan, gitInit: vi.fn(async () => ({ commit: 'a', files: 12, wrote_gitignore: true })) } });
    await mount(d, at);
    await tap('page', false);
    const step = host.querySelector('[data-testid="git-step"]')!;
    expect(step.textContent).toContain(T.needsGitTitle);
    expect(step.textContent).toContain('git init');
    expect(button('Continue')!.disabled).toBe(true);
    await tap(T.setUpGit);
    expect(d.ship.gitInit).toHaveBeenCalledWith('D:\\code\\page');
    expect(host.querySelector('[data-testid="git-step"]')).toBeNull();
    expect(button('Continue')!.disabled).toBe(false);
  });
});

describe('4. A first task', () => {
  const at: Seed = { start: 'task', look: 'code', project: { path: 'D:/code/shop', name: 'shop', state: 'repo' }, agentId: 'claude' };

  it('shows the suggested prompt before anything runs', async () => {
    const d = deps();
    await mount(d, at);
    expect(host.querySelector('[data-testid="task-prompt"]')!.textContent).toContain('Change nothing');
    expect(text()).toContain(T.taskSafe);
    expect(d.first.startTask).not.toHaveBeenCalled();
    expect(button('Continue')!.disabled).toBe(true);
  });

  it('runs it on the chosen folder and agent, then shows the answer, the receipt and the two hints', async () => {
    const d = deps();
    await mount(d, at);
    await tap(T.taskRun);
    expect(d.first.startTask).toHaveBeenCalledWith('D:/code/shop', 'claude');
    expect(host.querySelector('[data-testid="task-done"]')).not.toBeNull();
    expect(host.querySelector('.fr-answer')!.textContent).toBe('It is a small shop.');
    const receipt = host.querySelector('[data-testid="task-receipt"]')!.textContent!;
    expect(receipt).toContain('Read-only');
    expect(receipt).toContain('Yes');
    expect(receipt).toContain('Files changed');
    expect(host.querySelector('[data-testid="tip-receipt"]')!.textContent).toBe(T.receiptTip);
    expect(host.querySelector('[data-testid="tip-undo"]')!.textContent).toContain('Undo');
    expect(button('Continue')!.disabled).toBe(false);
  });

  it('while it runs it says so and Continue waits', async () => {
    vi.useFakeTimers();
    const d = deps();
    let n = 0;
    d.first.taskStatus = vi.fn(async () =>
      ++n < 2
        ? { status: 'running', result: null, error: null, receipt: null, pane_id: null }
        : { status: 'completed', result: 'Done.', error: null, receipt: null, pane_id: null },
    );
    await mount(d, at);
    await tap(T.taskRun);
    expect(text()).toContain(T.taskWorking('Claude Code'));
    expect(button('Continue')!.disabled).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1600);
    });
    expect(host.querySelector('[data-testid="task-done"]')).not.toBeNull();
    expect(button('Continue')!.disabled).toBe(false);
  });

  it('a failed task says your files were not touched and offers Try again', async () => {
    const d = deps();
    d.first.taskStatus = vi.fn(async () => ({ status: 'failed', result: null, error: { code: 'AGENT_EXITED' }, receipt: null, pane_id: null }));
    await mount(d, at);
    await tap(T.taskRun);
    const failed = host.querySelector('[data-testid="task-failed"]')!;
    expect(failed.textContent).toContain(T.taskFailed);
    expect(failed.textContent).toContain('stopped unexpectedly');
    await tap(T.taskTryAgain);
    expect(host.querySelector('[data-testid="task-prompt"]')).not.toBeNull();
  });

  it('a refusal from Rust shows a plain sentence, not the code', async () => {
    const d = deps();
    d.first.startTask = vi.fn(async () => {
      throw new Error('FOLDER_TOO_BROAD: C:/Users/Ana is not a folder Simple mode works in (home).');
    });
    await mount(d, at);
    await tap(T.taskRun);
    expect(host.querySelector('.fr-problem')!.textContent).toBe(T.tooBroad);
  });

  it('with no ready agent it says what to do and can go back to the agents', async () => {
    await mount(deps(), { ...at, agentId: null });
    expect(text()).toContain(T.taskNeedsAgent);
    await tap(T.stepNames.agents);
    expect(text()).toContain(T.agentsTitle);
  });

  it('can be skipped, and the welcome goes on', async () => {
    await mount(deps(), at);
    await tap(T.skipStep);
    expect(text()).toContain(T.phoneTitle);
  });

  it('an agent that asks first shows the approval card inline', async () => {
    const d = deps();
    d.first.startTask = vi.fn(async (project: string) => ({ project, handle: 'h1', pane_id: 'pane-9', enforced: false }));
    d.first.taskStatus = vi.fn(async () => ({ status: 'waiting_input', result: null, error: null, receipt: null, pane_id: 'pane-9' }));
    const c = cockpit({
      prompts: {
        'pane-9': {
          promptId: 'p1',
          kind: 'approval',
          text: 'Run npm test?',
          choices: [{ id: '1', label: 'Yes' }, { id: '3', label: 'No' }],
          risk: { level: 'low', headline: 'Run tests', reasons: ['It runs your tests.'] },
        },
      },
    });
    await mount(d, at, c);
    await tap(T.taskRun);
    expect(host.textContent).toContain('Run tests');
    expect(host.textContent).toContain(T.approvalTip);
    expect(host.textContent).toContain(T.taskAsking('Claude Code'));
  });
});

describe('the last steps', () => {
  it('Later skips the phone and the pack; the done step has three tips and finishes', async () => {
    const d = deps();
    const { onFinish } = await mount(d, { ...SEED, start: 'phone', look: 'code' });
    await tap(T.later);
    expect(text()).toContain(T.packTitle);
    await tap(T.later);
    expect(text()).toContain(T.doneTitle);
    expect(host.querySelectorAll('.fr-tipr')).toHaveLength(3);
    expect(text()).toContain('Ctrl+K');
    expect(text()).toContain('Inbox');
    expect(text()).toContain('History');
    expect(onFinish).not.toHaveBeenCalled();
    await tap(T.doneButton);
    expect(onFinish).toHaveBeenCalledTimes(1);
  });
});
