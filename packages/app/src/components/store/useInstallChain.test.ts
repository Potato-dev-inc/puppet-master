// The install chain: each step runs in a visible window, the chain waits for its end, and it stops at the first
// problem. Tested with fake store and install clients (no invoke) and fake timers for the 1 s poll.
import { describe, expect, it, vi } from 'vitest';
import type { AgentInstallApi, InstallFinish, InstallStatus, InstallStep } from '../../lib/agent-install';
import type { AgentStoreApi } from '../../lib/agent-store';
import {
  POLL_MS,
  chainReducer,
  runInstallChain,
  runUninstallChain,
  type ChainDeps,
  type ChainEvent,
  type InstallRun,
} from './useInstallChain';

const step = (over: Partial<InstallStep> = {}): InstallStep => ({
  index: 0,
  label: 'Qwen Code',
  tool: null,
  method: 'npm',
  command: 'npm install -g @qwen-code/qwen-code',
  source: 'https://vendor.example/docs',
  source_host: 'vendor.example',
  high_risk: false,
  note: '',
  ...over,
});

const done = (over: Partial<InstallStatus> = {}): InstallStatus => ({
  known: true,
  done: true,
  exit_code: 0,
  message: null,
  ...over,
});

const health = (installed: boolean) =>
  ({
    id: 'qwen',
    label: 'Qwen Code',
    state: installed ? 'unchecked' : 'not_installed',
    installed,
    version: installed ? '0.26.2' : null,
    reason: '',
    install_hint: null,
    login_hint: null,
    can_test: installed,
  }) as unknown as InstallFinish['health'];

const finishOk = (): InstallFinish => ({ path_added: [], health: health(true), needs_restart: false });

function fakeStore(over: Partial<AgentStoreApi> = {}): AgentStoreApi {
  return {
    list: vi.fn(),
    detail: vi.fn(),
    add: vi.fn(async () => ({ ok: true, id: 'qwen', state: 'ready' as const })),
    remove: vi.fn(),
    recordInstall: vi.fn(async () => undefined),
    uninstallPlan: vi.fn(),
    uninstallRun: vi.fn(async () => ({ pane_id: 'u1', command: 'x', label: 'Qwen Code' })),
    recordCheck: vi.fn(),
    ...over,
  };
}

function fakeInstall(statuses: InstallStatus[] = [done()], over: Partial<AgentInstallApi> = {}): AgentInstallApi {
  let call = 0;
  return {
    plan: vi.fn(),
    run: vi.fn(async (_id: string, index: number) => ({ pane_id: `p${index}`, command: 'x', label: 'x' })),
    status: vi.fn(async () => statuses[Math.min(call++, statuses.length - 1)]),
    finish: vi.fn(async () => finishOk()),
    ...over,
  };
}

function deps(store: AgentStoreApi, install: AgentInstallApi, over: Partial<ChainDeps> = {}): ChainDeps {
  return {
    store,
    install,
    sleep: async () => undefined,
    cancelled: () => false,
    report: () => undefined,
    ...over,
  };
}

const run = (over: Partial<InstallRun> = {}): InstallRun => ({
  id: 'qwen',
  label: 'Qwen Code',
  steps: [step()],
  acknowledgedRisk: false,
  alreadyAdded: false,
  ambiguous: false,
  ...over,
});

describe('chainReducer', () => {
  it('shows progress while a step runs, then the stop outcome', () => {
    const progress: ChainEvent = { type: 'progress', verb: 'Installing', step: 1, of: 2, label: 'Node.js' };
    const running = chainReducer({ phase: 'idle' }, progress);
    expect(running).toEqual({ phase: 'running', verb: 'Installing', step: 1, of: 2, label: 'Node.js' });
    const stopped = chainReducer(running, { type: 'stop', outcome: { tone: 'ok', text: 'Done.' } });
    expect(stopped).toEqual({ phase: 'stopped', outcome: { tone: 'ok', text: 'Done.' } });
  });
});

describe('runInstallChain', () => {
  it('runs the one step, records it and adds the agent', async () => {
    const store = fakeStore();
    const install = fakeInstall();
    const outcome = await runInstallChain(run(), deps(store, install));
    expect(install.run).toHaveBeenCalledWith('qwen', 0, 'npm install -g @qwen-code/qwen-code', false);
    expect(install.finish).toHaveBeenCalledTimes(1);
    expect(store.recordInstall).toHaveBeenCalledWith('qwen', 0);
    expect(store.add).toHaveBeenCalledWith('qwen', true);
    expect(outcome).toEqual({ tone: 'ok', text: 'Installed and added. Open Qwen Code to sign in.' });
  });

  it('runs a prerequisite first, then the agent, and records the agent step', async () => {
    const store = fakeStore();
    const install = fakeInstall([done(), done()]);
    const node = step({ index: 0, label: 'Node.js', tool: 'node', method: 'winget', command: 'winget install node' });
    const agent = step({ index: 1 });
    const outcome = await runInstallChain(run({ steps: [node, agent] }), deps(store, install));
    expect(vi.mocked(install.run).mock.calls.map((c) => c[1])).toEqual([0, 1]);
    expect(install.finish).toHaveBeenCalledTimes(2);
    expect(store.recordInstall).toHaveBeenCalledWith('qwen', 1);
    expect(store.add).toHaveBeenCalledTimes(1);
    expect(outcome.tone).toBe('ok');
  });

  it('stops at a failed step: no finish, no record, no add', async () => {
    const store = fakeStore();
    const install = fakeInstall([done({ exit_code: 1, message: 'Access denied.' })]);
    const outcome = await runInstallChain(run(), deps(store, install));
    expect(outcome).toEqual({ tone: 'problem', text: 'Access denied. Your PC is fine. You can try again.' });
    expect(install.finish).not.toHaveBeenCalled();
    expect(store.recordInstall).not.toHaveBeenCalled();
    expect(store.add).not.toHaveBeenCalled();
  });

  it('never runs a script step after another step', async () => {
    const store = fakeStore();
    const install = fakeInstall([done()]);
    const node = step({ index: 0, tool: 'node', method: 'winget', command: 'winget install node' });
    const script = step({ index: 1, method: 'script', high_risk: true, source_host: 'get.example', command: 'irm x | iex' });
    const outcome = await runInstallChain(run({ steps: [node, script], acknowledgedRisk: true }), deps(store, install));
    expect(install.run).toHaveBeenCalledTimes(1);
    expect(outcome.tone).toBe('problem');
    expect(outcome.text).toBe('The next step downloads a script from get.example. Press Install again to see it and confirm.');
  });

  it('does not run a script step without the trust box', async () => {
    const store = fakeStore();
    const install = fakeInstall();
    const script = step({ method: 'script', high_risk: true, source_host: 'get.example' });
    const outcome = await runInstallChain(run({ steps: [script], acknowledgedRisk: false }), deps(store, install));
    expect(install.run).not.toHaveBeenCalled();
    expect(outcome.tone).toBe('problem');
  });

  it('runs a first script step once the trust box is ticked', async () => {
    const store = fakeStore();
    const install = fakeInstall();
    const script = step({ method: 'script', high_risk: true });
    await runInstallChain(run({ steps: [script], acknowledgedRisk: true }), deps(store, install));
    expect(install.run).toHaveBeenCalledWith('qwen', 0, script.command, true);
  });

  it('does not add an agent that is already on the list', async () => {
    const store = fakeStore();
    const outcome = await runInstallChain(run({ alreadyAdded: true }), deps(store, fakeInstall()));
    expect(store.recordInstall).toHaveBeenCalledWith('qwen', 0);
    expect(store.add).not.toHaveBeenCalled();
    expect(outcome).toEqual({ tone: 'ok', text: 'Installed. Qwen Code is ready. Open it to sign in.' });
  });

  it('waits for the Add press when the program name is ambiguous', async () => {
    const store = fakeStore();
    const outcome = await runInstallChain(run({ ambiguous: true }), deps(store, fakeInstall()));
    expect(store.add).not.toHaveBeenCalled();
    expect(outcome.text).toBe('Installed. Press Add to confirm the program Puppet Master found for Qwen Code.');
  });

  it('asks for a restart when the program is installed but not found yet', async () => {
    const store = fakeStore();
    const install = fakeInstall([done()], {
      finish: vi.fn(async () => ({ path_added: [], health: health(false), needs_restart: true })),
    });
    const outcome = await runInstallChain(run(), deps(store, install));
    expect(outcome).toEqual({
      tone: 'problem',
      text: 'Installed, but Puppet Master cannot find it yet. Close Puppet Master and open it again.',
    });
    expect(store.recordInstall).not.toHaveBeenCalled();
    expect(store.add).not.toHaveBeenCalled();
  });

  it('reports a finish that found nothing and no restart', async () => {
    const install = fakeInstall([done()], {
      finish: vi.fn(async () => ({ path_added: [], health: health(false), needs_restart: false })),
    });
    const outcome = await runInstallChain(run(), deps(fakeStore(), install));
    expect(outcome.text).toBe('The install finished, but Puppet Master cannot find Qwen Code yet. You can try again.');
  });

  it('stops when the window closes before its command ends', async () => {
    const install = fakeInstall([{ known: true, done: false, exit_code: null, message: null, closed: true }]);
    const outcome = await runInstallChain(run(), deps(fakeStore(), install));
    expect(outcome).toEqual({
      tone: 'problem',
      text: 'The install window closed before it finished. You can try again.',
    });
  });

  it('starts nothing once the sheet is gone', async () => {
    const install = fakeInstall();
    const outcome = await runInstallChain(run(), deps(fakeStore(), install, { cancelled: () => true }));
    expect(install.run).not.toHaveBeenCalled();
    expect(outcome).toEqual({ tone: 'problem', text: 'Stopped.' });
  });

  it('turns a rejected add into its plain sentence', async () => {
    const store = fakeStore({
      add: vi.fn(async () => {
        throw 'AGENT_DEF_DUPLICATE_ID: A file for this agent already exists.';
      }),
    });
    const outcome = await runInstallChain(run(), deps(store, fakeInstall()));
    expect(outcome).toEqual({ tone: 'problem', text: 'A file for this agent already exists.' });
  });

  it('polls once a second until the window ends', async () => {
    vi.useFakeTimers();
    try {
      const install = fakeInstall([
        { known: true, done: false, exit_code: null, message: null },
        { known: true, done: false, exit_code: null, message: null },
        done(),
      ]);
      const pending = runInstallChain(run(), deps(fakeStore(), install, { sleep: (ms) => new Promise((r) => setTimeout(r, ms)) }));
      await vi.advanceTimersByTimeAsync(POLL_MS * 2);
      const outcome = await pending;
      expect(install.status).toHaveBeenCalledTimes(3);
      expect(outcome.tone).toBe('ok');
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('runUninstallChain', () => {
  const uninstall = { id: 'qwen', label: 'Qwen Code', step: step({ command: 'npm uninstall -g @qwen-code/qwen-code' }) };

  it('runs the shown command, checks the health and says what stayed', async () => {
    const store = fakeStore();
    const install = fakeInstall();
    const outcome = await runUninstallChain(uninstall, deps(store, install));
    expect(store.uninstallRun).toHaveBeenCalledWith('qwen', 0, 'npm uninstall -g @qwen-code/qwen-code');
    expect(install.finish).toHaveBeenCalledWith('qwen');
    expect(outcome).toEqual({ tone: 'ok', text: 'Uninstalled Qwen Code. Sign-in data and sessions it kept are not removed.' });
  });

  it('shows the failure and does not check the health', async () => {
    const store = fakeStore();
    const install = fakeInstall([done({ exit_code: 2, message: null })]);
    const outcome = await runUninstallChain(uninstall, deps(store, install));
    expect(outcome.tone).toBe('problem');
    expect(install.finish).not.toHaveBeenCalled();
  });
});
