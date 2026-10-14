// @vitest-environment jsdom
// Install in "Set up agents": the exact command is shown first, nothing runs until Run is pressed, the
// install goes on in a visible window, and the row is checked again when the window's command ends.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentHealth, AgentHealthApi } from '../../lib/agent-health';
import type { AgentInstallApi, InstallPlan, InstallStep } from '../../lib/agent-install';
import { AgentDoctor, resetKeptInstalls } from './AgentDoctor';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  resetKeptInstalls();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const missing: AgentHealth = {
  id: 'tool',
  label: 'Tool',
  state: 'not_installed',
  installed: false,
  version: null,
  reason: 'Not installed on this computer.',
  install_hint: 'npm i -g tool',
  login_hint: 'Run: tool login',
  can_test: false,
};
const installed: AgentHealth = {
  ...missing,
  state: 'unchecked',
  installed: true,
  version: '1.0.0',
  reason: 'Installed. Press Test to check it can answer.',
  can_test: true,
};

const step = (over: Partial<InstallStep> = {}): InstallStep => ({
  index: 0,
  label: 'Tool',
  tool: null,
  method: 'winget',
  command: 'winget install --id Vendor.Tool -e --source winget',
  source: 'https://vendor.example/docs/setup',
  source_host: 'vendor.example',
  high_risk: false,
  note: '',
  ...over,
});
const plan = (steps: InstallStep[], over: Partial<InstallPlan> = {}): InstallPlan => ({
  agent_id: 'tool',
  label: 'Tool',
  available: true,
  reason: null,
  hint: 'npm i -g tool',
  steps,
  ...over,
});

function apis(over: Partial<AgentInstallApi> = {}, health: AgentHealth[] = [missing]) {
  const healthApi: AgentHealthApi = { check: async () => health, test: vi.fn() };
  const installApi: AgentInstallApi = {
    plan: vi.fn(async () => plan([step()])),
    run: vi.fn(async () => ({ pane_id: 'pane-1', command: 'x', label: 'Tool' })),
    status: vi.fn(async () => ({ known: true, done: false, exit_code: null, message: null })),
    finish: vi.fn(async () => ({ path_added: [], health: installed, needs_restart: false })),
    ...over,
  };
  return { healthApi, installApi };
}

const flush = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
const button = (text: string) =>
  Array.from(document.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement | undefined;
const click = async (text: string) => {
  await act(async () => button(text)!.click());
  await flush();
};
const tick = async (ms = 1600) => {
  await act(async () => {
    vi.advanceTimersByTime(ms);
  });
  await flush();
};

async function mount(a: ReturnType<typeof apis>, extra: Partial<Parameters<typeof AgentDoctor>[0]> = {}) {
  await act(async () => {
    root.render(<AgentDoctor api={a.healthApi} installApi={a.installApi} onOpenAgent={() => {}} {...extra} />);
  });
  await flush();
}

describe('Install: confirm before run', () => {
  it('shows the exact command, where it comes from and what to expect, and runs nothing yet', async () => {
    const a = apis();
    await mount(a);
    await click('Install');
    const sheet = document.querySelector('.ck-sheet--install') as HTMLElement;
    expect(sheet).not.toBeNull();
    expect(sheet.querySelector('[data-testid="install-command"]')!.textContent).toBe(
      'winget install --id Vendor.Tool -e --source winget',
    );
    expect(sheet.textContent).toContain('This comes from vendor.example.');
    expect(sheet.textContent).toContain('in a window you can watch');
    expect(sheet.textContent).toContain('Windows may ask if the installer can make changes');
    expect(a.installApi.plan).toHaveBeenCalledWith('tool');
    expect(a.installApi.run).not.toHaveBeenCalled();
  });

  it('Cancel closes the sheet and still runs nothing', async () => {
    const a = apis();
    await mount(a);
    await click('Install');
    await click('Cancel');
    expect(document.querySelector('.ck-sheet--install')).toBeNull();
    expect(a.installApi.run).not.toHaveBeenCalled();
  });

  it('Run sends the step and the very command that was shown', async () => {
    const a = apis();
    await mount(a);
    await click('Install');
    await click('Run');
    expect(a.installApi.run).toHaveBeenCalledWith('tool', 0, 'winget install --id Vendor.Tool -e --source winget', false);
    expect(document.querySelector('.ck-sheet--install')).toBeNull();
  });

  it('copies the command from the sheet', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const a = apis();
    await mount(a, { onNotify: vi.fn() });
    await click('Install');
    await click('Copy command');
    expect(writeText).toHaveBeenCalledWith('winget install --id Vendor.Tool -e --source winget');
  });

  it('a refused run (the command changed) is explained in the sheet and nothing starts', async () => {
    const a = apis({
      run: vi.fn(async () => {
        throw new Error('INSTALL_COMMAND_CHANGED: the command is not the one you were shown. Open Install again.');
      }),
    });
    await mount(a);
    await click('Install');
    await click('Run');
    const sheet = document.querySelector('.ck-sheet--install') as HTMLElement;
    expect(sheet.textContent).toContain('not the one you were shown');
    expect(sheet.textContent).not.toContain('INSTALL_COMMAND_CHANGED');
    expect(host.textContent).not.toContain('Installing');
  });
});

describe('Install: a script from the internet needs a second yes', () => {
  it('Run stays off until the trust box is ticked, then sends the confirmation', async () => {
    const a = apis({
      plan: vi.fn(async () =>
        plan([step({ method: 'script', high_risk: true, command: "irm 'https://vendor.example/i.ps1' | iex" })]),
      ),
    });
    await mount(a);
    await click('Install');
    expect(button('Run')!.disabled).toBe(true);
    expect(document.body.textContent).toContain('downloads a script from the internet');
    expect(document.body.textContent).toContain('I trust vendor.example');
    await act(async () => (document.querySelector('.ck-install-risk input') as HTMLInputElement).click());
    expect(button('Run')!.disabled).toBe(false);
    await click('Run');
    expect(a.installApi.run).toHaveBeenCalledWith('tool', 0, "irm 'https://vendor.example/i.ps1' | iex", true);
  });
});

describe('Install: nothing to run from here', () => {
  it('says why, offers the old command to copy and has no Run button', async () => {
    const a = apis({
      plan: vi.fn(async () =>
        plan([], { available: false, reason: 'This PC cannot install apps from the command line (winget is missing).' }),
      ),
    });
    await mount(a);
    await click('Install');
    const sheet = document.querySelector('.ck-sheet--install, .ck-sheet') as HTMLElement;
    expect(sheet.textContent).toContain('winget is missing');
    expect(sheet.querySelector('code')!.textContent).toBe('npm i -g tool');
    expect(button('Run')).toBeUndefined();
    expect(button('Copy command')).toBeDefined();
  });
});

describe('Install: running in a visible window, then checking again', () => {
  it('shows Installing with a way to the window, polls, then refreshes the row and offers Sign in', async () => {
    const onShowPane = vi.fn();
    let done = false;
    const a = apis({
      status: vi.fn(async () => ({ known: true, done, exit_code: done ? 0 : null, message: null })),
    });
    await mount(a, { onShowPane });
    await click('Install');
    await click('Run');
    expect(host.textContent).toContain('Installing');
    expect(host.textContent).toContain('Installing Tool in the window');
    await click('Show window');
    expect(onShowPane).toHaveBeenCalledWith('pane-1');
    await tick();
    expect(a.installApi.status).toHaveBeenCalledWith('pane-1');
    expect(a.installApi.finish).not.toHaveBeenCalled();
    done = true;
    await tick();
    expect(a.installApi.finish).toHaveBeenCalledWith('tool');
    expect(host.textContent).toContain('Installed. Sign in next.');
    expect(host.textContent).toContain('1.0.0');
    expect(host.textContent).toContain('Puppet Master never sees your password');
    expect(button('Sign in')).toBeDefined();
    expect(button('Install')).toBeUndefined();
  });

  it('Sign in opens the agent in a visible pane (the app never types credentials)', async () => {
    const onOpenAgent = vi.fn();
    const a = apis({}, [installed]);
    await mount(a, { onOpenAgent });
    await click('Sign in');
    expect(onOpenAgent).toHaveBeenCalledWith('tool');
  });

  it('a cancelled Windows prompt is said plainly and Install is offered again', async () => {
    const a = apis({
      status: vi.fn(async () => ({
        known: true,
        done: true,
        exit_code: 1223,
        message: 'The install was cancelled in the Windows prompt.',
      })),
      finish: vi.fn(async () => ({ path_added: [], health: missing, needs_restart: false })),
    });
    await mount(a);
    await click('Install');
    await click('Run');
    await tick();
    expect(host.textContent).toContain('cancelled in the Windows prompt');
    expect(host.textContent).toContain('Your PC is fine');
    expect(button('Install')).toBeDefined();
  });

  it('installed on disk but not found after the PATH refresh: asks for a restart', async () => {
    const a = apis({
      status: vi.fn(async () => ({ known: true, done: true, exit_code: 0, message: null })),
      finish: vi.fn(async () => ({ path_added: [], health: missing, needs_restart: true })),
    });
    await mount(a);
    await click('Install');
    await click('Run');
    await tick();
    expect(host.textContent).toContain('cannot find it yet');
    expect(host.textContent).toContain('open it again');
  });

  it('a prerequisite step that succeeded leads straight to the next confirmation', async () => {
    const node = step({ label: 'Node.js', tool: 'node', command: 'winget install --id OpenJS.NodeJS.LTS -e --source winget' });
    const npm = step({ index: 1, method: 'npm', command: 'npm install -g tool' });
    const planCalls: InstallPlan[] = [plan([node, npm]), plan([{ ...npm, index: 0 }])];
    const a = apis({
      plan: vi.fn(async () => planCalls.shift()!),
      status: vi.fn(async () => ({ known: true, done: true, exit_code: 0, message: null })),
      finish: vi.fn(async () => ({ path_added: ['C:\\Program Files\\nodejs'], health: missing, needs_restart: true })),
    });
    await mount(a);
    await click('Install');
    expect(document.body.textContent).toContain('Install Node.js');
    expect(document.body.textContent).toContain('Tool needs Node.js first. This is step 1 of 2.');
    expect(document.body.textContent).toContain('npm install -g tool');
    await click('Run');
    expect(a.installApi.run).toHaveBeenCalledWith('tool', 0, node.command, false);
    await tick();
    expect(a.installApi.plan).toHaveBeenCalledTimes(2);
    expect(document.querySelector('[data-testid="install-command"]')!.textContent).toBe('npm install -g tool');
    expect(document.body.textContent).not.toContain('step 1 of');
  });

  it('the install survives leaving and coming back to the screen', async () => {
    const a = apis();
    await mount(a);
    await click('Install');
    await click('Run');
    act(() => root.unmount());
    root = createRoot(host);
    await mount(a);
    expect(host.textContent).toContain('Installing');
  });
});

describe('Install: copy and states follow UX-PHASE1', () => {
  it('uses the plain words and never the (+) glyph', async () => {
    const a = apis();
    await mount(a);
    expect(host.querySelector('[aria-label="Needs you"]')).toBeNull();
    await click('Install');
    await click('Run');
    expect(host.querySelector('[aria-label="Needs you"]')).toBeNull();
    for (const bad of ['child', 'pane', 'winget_id', 'PATH', 'env']) {
      expect(host.textContent).not.toContain(bad);
    }
  });
});
