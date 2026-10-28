// The steps of one Agent Store install or uninstall, run one after another in visible windows.
// A step runs, the chain waits until its command ends, then PATH is refreshed (`agents_install_finish`).
// The chain stops at the first problem. A script step never runs after another step: the chain stops and
// asks the person to press Install again, so they see the script and confirm it on the sheet.
import { useEffect, useReducer, useRef } from 'react';
import {
  agentInstallApi,
  type AgentInstallApi,
  type InstallFinish,
  type InstallStatus,
  type InstallStep,
} from '../../lib/agent-install';
import { agentStoreApi, type AgentStoreApi } from '../../lib/agent-store';

/** How often the chain asks whether a window's command has ended. */
export const POLL_MS = 1000;

export interface ChainOutcome {
  tone: 'ok' | 'problem';
  /** Plain sentence for the sheet. */
  text: string;
}

export type ChainState =
  | { phase: 'idle' }
  | { phase: 'running'; verb: 'Installing' | 'Uninstalling'; step: number; of: number; label: string }
  | { phase: 'stopped'; outcome: ChainOutcome };

export type ChainEvent =
  | { type: 'progress'; verb: 'Installing' | 'Uninstalling'; step: number; of: number; label: string }
  | { type: 'stop'; outcome: ChainOutcome };

export function chainReducer(_state: ChainState, event: ChainEvent): ChainState {
  switch (event.type) {
    case 'progress':
      return { phase: 'running', verb: event.verb, step: event.step, of: event.of, label: event.label };
    case 'stop':
      return { phase: 'stopped', outcome: event.outcome };
  }
}

/** The error text the app's commands send: "CODE: sentence" becomes the sentence. */
export function errText(e: unknown): string {
  return (e instanceof Error ? e.message : String(e)).replace(/^[A-Z_]+: /, '');
}

export interface ChainDeps {
  store: AgentStoreApi;
  install: AgentInstallApi;
  sleep: (ms: number) => Promise<void>;
  /** True once the sheet is gone: the chain starts no more steps. */
  cancelled: () => boolean;
  report: (event: ChainEvent) => void;
}

export interface InstallRun {
  id: string;
  label: string;
  steps: InstallStep[];
  /** True only after the person ticked the trust box for a script step. */
  acknowledgedRisk: boolean;
  /** The file is already on the list (state added_no_program): do not add it again. */
  alreadyAdded: boolean;
  /** The program name is ambiguous: the person must press Add to confirm the program found. */
  ambiguous: boolean;
}

export interface UninstallRun {
  id: string;
  label: string;
  step: InstallStep;
}

const CANCELLED: ChainOutcome = { tone: 'problem', text: 'Stopped.' };

const fail = (text: string): ChainOutcome => ({ tone: 'problem', text });

function failedText(status: InstallStatus): string {
  if (status.closed && !status.done) return 'The install window closed before it finished. You can try again.';
  return `${status.message ?? 'The install did not finish.'} Your PC is fine. You can try again.`;
}

/** Asks every POLL_MS until the window's command has ended. Null when the sheet went away first. */
async function waitForEnd(deps: ChainDeps, paneId: string): Promise<InstallStatus | null> {
  for (;;) {
    if (deps.cancelled()) return null;
    const status = await deps.install.status(paneId);
    if (status.done || status.closed || !status.known) return status;
    await deps.sleep(POLL_MS);
  }
}

/** Install every step in order, record what was installed, then add the agent (unless it is already on the list). */
export async function runInstallChain(run: InstallRun, deps: ChainDeps): Promise<ChainOutcome> {
  try {
    let finish: InstallFinish | null = null;
    for (let i = 0; i < run.steps.length; i++) {
      const step = run.steps[i];
      if (deps.cancelled()) return CANCELLED;
      if (step.high_risk && (i > 0 || !run.acknowledgedRisk)) {
        return fail(
          `The next step downloads a script from ${step.source_host}. Press Install again to see it and confirm.`,
        );
      }
      deps.report({ type: 'progress', verb: 'Installing', step: i + 1, of: run.steps.length, label: step.label });
      const started = await deps.install.run(run.id, step.index, step.command, step.high_risk);
      const status = await waitForEnd(deps, started.pane_id);
      if (!status) return CANCELLED;
      if (!status.done || status.exit_code !== 0) return fail(failedText(status));
      finish = await deps.install.finish(run.id);
    }
    if (!finish) return fail('There is nothing to install.');
    if (!finish.health.installed) {
      return finish.needs_restart
        ? fail('Installed, but Puppet Master cannot find it yet. Close Puppet Master and open it again.')
        : fail(`The install finished, but Puppet Master cannot find ${run.label} yet. You can try again.`);
    }
    const agent = run.steps.find((s) => s.tool === null) ?? run.steps[run.steps.length - 1];
    await deps.store.recordInstall(run.id, agent.index);
    if (run.alreadyAdded) return { tone: 'ok', text: `Installed. ${run.label} is ready. Open it to sign in.` };
    if (run.ambiguous) {
      return { tone: 'ok', text: `Installed. Press Add to confirm the program Puppet Master found for ${run.label}.` };
    }
    await deps.store.add(run.id, true);
    return { tone: 'ok', text: `Installed and added. Open ${run.label} to sign in.` };
  } catch (e) {
    return fail(errText(e));
  }
}

/** Run the one uninstall step. The program goes; Puppet Master's file for the agent stays. */
export async function runUninstallChain(run: UninstallRun, deps: ChainDeps): Promise<ChainOutcome> {
  try {
    if (deps.cancelled()) return CANCELLED;
    deps.report({ type: 'progress', verb: 'Uninstalling', step: 1, of: 1, label: run.step.label });
    const started = await deps.store.uninstallRun(run.id, run.step.index, run.step.command);
    const status = await waitForEnd(deps, started.pane_id);
    if (!status) return CANCELLED;
    if (!status.done || status.exit_code !== 0) return fail(failedText(status));
    await deps.install.finish(run.id);
    return { tone: 'ok', text: `Uninstalled ${run.label}. Sign-in data and sessions it kept are not removed.` };
  } catch (e) {
    return fail(errText(e));
  }
}

export interface InstallChainOptions {
  store?: AgentStoreApi;
  install?: AgentInstallApi;
  sleep?: (ms: number) => Promise<void>;
}

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** The chain as a React hook: `state` drives the progress view, the promise gives the outcome to show. */
export function useInstallChain(options: InstallChainOptions = {}) {
  const [state, dispatch] = useReducer(chainReducer, { phase: 'idle' } as ChainState);
  const aliveRef = useRef(true);
  const optionsRef = useRef(options);
  optionsRef.current = options;
  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  const deps = (): ChainDeps => ({
    store: optionsRef.current.store ?? agentStoreApi,
    install: optionsRef.current.install ?? agentInstallApi,
    sleep: optionsRef.current.sleep ?? wait,
    cancelled: () => !aliveRef.current,
    report: dispatch,
  });

  return {
    state,
    install: async (run: InstallRun): Promise<ChainOutcome> => {
      const outcome = await runInstallChain(run, deps());
      if (aliveRef.current) dispatch({ type: 'stop', outcome });
      return outcome;
    },
    uninstall: async (run: UninstallRun): Promise<ChainOutcome> => {
      const outcome = await runUninstallChain(run, deps());
      if (aliveRef.current) dispatch({ type: 'stop', outcome });
      return outcome;
    },
  };
}
