// The first-run welcome: seven steps over the cockpit, shown once on a fresh profile, skippable at every
// step, and reopened from the palette or Settings. It draws `model.ts`; the choices that matter reach
// Rust through the ordinary settings patch (look, step, folder) and `first_run_*` (the first task).
import { useCallback, useEffect, useReducer, useRef, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import { agentHealthApi, type AgentHealthApi } from '../../lib/agent-health';
import { agentInstallApi, type AgentInstallApi } from '../../lib/agent-install';
import { agentPackApi, type AgentPackApi } from '../../lib/agent-pack';
import { loadSettings, patchSettings } from '../../lib/settings';
import { shipApi, type ShipApi } from '../../lib/ship-api';
import { simpleApi, type SimpleApi } from '../simple/simple-api';
import { DotSpinner } from '../cockpit/DotSpinner';
import '../../styles/first-run.css';
import { firstRunApi, isFinalStatus, type FirstRunApi, type TaskStarted, type TaskStatus } from './first-run-api';
import { T } from './copy';
import {
  agentPatch,
  canContinue,
  initialFlow,
  lookPatch,
  projectPatch,
  reduce,
  stepIndex,
  stepPatch,
  STEPS,
  type Look,
  type StepId,
} from './model';
import type { Seed } from './useFirstRun';
import { StepAgents } from './StepAgents';
import { StepProject } from './StepProject';
import { StepTask, type TaskView } from './StepTask';
import { StepDone, StepPack, StepPhone, StepWelcome } from './StepsSmall';

/** What the welcome talks to. Tests and the harness pass fakes; the app passes nothing. */
export interface FirstRunDeps {
  first: FirstRunApi;
  ship: Pick<ShipApi, 'recent' | 'pickFolder' | 'forget' | 'gitInitPlan' | 'gitInit'>;
  simple: Pick<SimpleApi, 'checkFolder'>;
  patch: typeof patchSettings;
  load: typeof loadSettings;
  health?: AgentHealthApi;
  install?: AgentInstallApi;
  pack?: AgentPackApi;
}

export function defaultDeps(): FirstRunDeps {
  return { first: firstRunApi, ship: shipApi, simple: simpleApi, patch: patchSettings, load: loadSettings, health: agentHealthApi, install: agentInstallApi, pack: agentPackApi };
}

const POLL_MS = 1500;

/** Runs `fn` when `value` differs from the last render's, never on the first render. */
function useWhenChanged<V>(value: V, fn: (value: V) => void): void {
  const last = useRef(value);
  useEffect(() => {
    if (Object.is(last.current, value)) return;
    last.current = value;
    fn(value);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);
}

function Progress({ step }: { step: StepId }) {
  const at = stepIndex(step);
  return (
    <ol className="fr-dots" aria-label={T.stepOf(at + 1, STEPS.length)}>
      {STEPS.map((id, i) => (
        <li key={id} className="fr-dot" data-state={i < at ? 'done' : i === at ? 'now' : 'next'} title={T.stepNames[id]} />
      ))}
    </ol>
  );
}

export function FirstRun({ c, seed, onFinish, deps }: { c: CockpitApi; seed: Seed; onFinish: () => void; deps?: Partial<FirstRunDeps> }) {
  const d: FirstRunDeps = { ...defaultDeps(), ...deps };
  const dRef = useRef(d);
  dRef.current = d;
  const listing = useAgentRegistry();
  const [flow, dispatch] = useReducer(reduce, undefined, () =>
    initialFlow({
      step: seed.start,
      look: seed.look,
      project: seed.project,
      ready: seed.agentId ? [{ id: seed.agentId, label: agentLabel(listing, seed.agentId) }] : [],
    }),
  );
  // Paused: the person went to a window (sign in, an install) and the welcome waits as a small bar.
  const [docked, setDocked] = useState(false);
  const [taskView, setTaskView] = useState<TaskView>({ run: null, status: null, startedAt: 0 });

  const save = useCallback((patch: Record<string, unknown>) => {
    void dRef.current.patch(patch as never).catch(() => undefined);
  }, []);

  // Where the person is, so a restart resumes there. Nothing is written until something changes.
  useWhenChanged(flow.step, (step) => save(stepPatch(step)));
  useWhenChanged(flow.project?.path ?? '', () => save(projectPatch(flow.project)));
  useWhenChanged(flow.ready.map((a) => a.id).join(','), () => save(agentPatch(flow.ready)));

  const finishedOnce = useRef(false);
  useEffect(() => {
    if (flow.finished && !finishedOnce.current) {
      finishedOnce.current = true;
      onFinish();
    }
  }, [flow.finished, onFinish]);

  // The first task keeps running (and being watched) when the person moves to another step.
  const run = taskView.run;
  const status = taskView.status;
  const watching = run !== null && !(status && isFinalStatus(status.status));
  useEffect(() => {
    if (!run || !watching) return;
    let alive = true;
    const look = () => {
      d.first
        .taskStatus(run.project, run.handle)
        .then((next: TaskStatus) => {
          if (!alive) return;
          setTaskView((v) => (v.run === run ? { ...v, status: next } : v));
          if (isFinalStatus(next.status)) dispatch({ type: 'task', task: next.status === 'completed' ? 'done' : 'failed' });
        })
        .catch(() => undefined);
    };
    look();
    const timer = window.setInterval(look, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [run, watching]);

  const chooseLook = (look: Look) => {
    dispatch({ type: 'look', look });
    // Written now; the cockpit keeps its normal face until the welcome closes (CockpitApp).
    save({ ...lookPatch(look) });
  };

  const dock = (then?: () => void) => {
    setDocked(true);
    then?.();
  };

  const step = flow.step;
  const optional = step === 'phone' || step === 'pack';
  const last = step === 'done';
  const primaryLabel = last ? T.doneButton : T.next;

  return (
    <div className="fr-layer" data-docked={docked ? 'true' : undefined} data-testid="first-run">
      {docked && (
        <div className="fr-bar" role="status">
          <DotSpinner status="waiting_input" />
          <span>{T.pausedBar}</span>
          <button type="button" className="ck-btn ck-btn--primary ck-btn--sm" onClick={() => setDocked(false)}>
            {T.pausedButton}
          </button>
        </div>
      )}
      <div className="fr-shell" role="dialog" aria-label={T.title} hidden={docked}>
        <header className="fr-top">
          <Progress step={step} />
          <span className="fr-stepof">{T.stepOf(stepIndex(step) + 1, STEPS.length)}</span>
          {!last && (
            <button type="button" className="ck-btn ck-btn--plain ck-btn--sm fr-skipall" onClick={() => dispatch({ type: 'skipAll' })}>
              {T.skipSetup}
            </button>
          )}
        </header>

        <main className="fr-body" key={step}>
          {step === 'welcome' && <StepWelcome look={flow.look} onLook={chooseLook} />}
          {step === 'agents' && (
            <StepAgents
              ready={flow.ready}
              api={d.health}
              installApi={d.install}
              onRows={(ready) => dispatch({ type: 'agents', ready })}
              onNotify={c.notify}
              onSignIn={(id) => dock(() => void c.newAgent(id))}
              onShowPane={(id) => dock(() => c.select(id))}
            />
          )}
          {step === 'project' && (
            <StepProject
              project={flow.project}
              ship={d.ship}
              simple={d.simple}
              onProject={(project) => dispatch({ type: 'project', project })}
            />
          )}
          {step === 'task' && (
            <StepTask
              c={c}
              flow={flow}
              view={taskView}
              first={d.first}
              onView={setTaskView}
              onTask={(task) => dispatch({ type: 'task', task })}
              onGo={(to) => dispatch({ type: 'go', step: to })}
              onOpenPane={(id) => dock(() => c.select(id))}
            />
          )}
          {step === 'phone' && <StepPhone load={d.load} patch={d.patch} />}
          {step === 'pack' && <StepPack api={d.pack} />}
          {step === 'done' && <StepDone />}
        </main>

        <footer className="fr-foot">
          <button type="button" className="ck-btn ck-btn--plain" disabled={stepIndex(step) === 0} onClick={() => dispatch({ type: 'back' })}>
            {T.back}
          </button>
          <span className="fr-foot-gap" />
          {step !== 'welcome' && !last && (
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => dispatch({ type: 'skip' })}>
              {optional ? T.later : T.skipStep}
            </button>
          )}
          <button type="button" className="ck-btn ck-btn--primary" disabled={!canContinue(flow)} onClick={() => dispatch({ type: 'next' })}>
            {primaryLabel}
          </button>
        </footer>
        <p className="fr-reopen">{T.reopenNote}</p>
      </div>
    </div>
  );
}

export type { TaskStarted };
