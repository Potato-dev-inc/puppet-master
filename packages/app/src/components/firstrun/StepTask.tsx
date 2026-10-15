// Step 4: a first task that cannot hurt. A suggested prompt, run in a separate copy, read-only wherever
// the agent can be locked to it. When it ends the person sees the answer, a receipt and a short note on
// what a receipt and Undo are (inline hints, not dialogs). An agent that asks first shows the same
// approval card as everywhere else.
import { useEffect, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { ApprovalPrompt } from '../cockpit/ApprovalPrompt';
import { DotSpinner } from '../cockpit/DotSpinner';
import { explainError, type ErrorExplanation } from '../cockpit/error-sentences';
import { formatElapsed } from '../cockpit/model';
import { readableReceipt } from '../cockpit/receipt-view';
import { codeOf, type FirstRunApi, type TaskStarted, type TaskStatus } from './first-run-api';
import { Answer } from './answer-text';
import { T, TASK_PROMPT } from './copy';
import type { Flow, StepId } from './model';

/** The first task as the shell keeps it, so it survives a trip to another step. */
export interface TaskView {
  run: TaskStarted | null;
  status: TaskStatus | null;
  startedAt: number;
}

/** The lines of the receipt this step shows. The full receipt is in each pane's Receipt tab. */
const RECEIPT_KEYS = ['Result', 'Read-only', 'Agent', 'Files changed'];

export function receiptLines(receipt: unknown): { key: string; value: string }[] {
  const lines = readableReceipt(receipt);
  return RECEIPT_KEYS.flatMap((key) => lines.filter((l) => l.key === key).slice(0, 1));
}

function useSecond(on: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!on) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [on]);
  return now;
}

export function StepTask({
  c,
  flow,
  view,
  first,
  onView,
  onTask,
  onGo,
  onOpenPane,
}: {
  c: CockpitApi;
  flow: Flow;
  view: TaskView;
  first: FirstRunApi;
  onView: (next: TaskView) => void;
  onTask: (task: Flow['task']) => void;
  onGo: (step: StepId) => void;
  onOpenPane: (paneId: string) => void;
}) {
  const agents = flow.ready;
  const project = flow.project;
  const [agentId, setAgentId] = useState(agents[0]?.id ?? '');
  const [starting, setStarting] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const { run, status } = view;
  const settled = status !== null && (status.status === 'completed' || status.status === 'failed' || status.status === 'cancelled');
  const working = run !== null && !settled;
  const now = useSecond(working);
  const agent = agents.find((a) => a.id === agentId) ?? agents[0];

  const start = async () => {
    if (!project || !agent) return;
    setStarting(true);
    setProblem(null);
    try {
      const started = await first.startTask(project.path, agent.id);
      onView({ run: started, status: null, startedAt: Date.now() });
      onTask('running');
    } catch (err) {
      const code = codeOf(err);
      setProblem(code === 'FOLDER_NEEDS_GIT' ? T.needsGitTitle : code === 'FOLDER_TOO_BROAD' ? T.tooBroad : T.taskFailed);
    } finally {
      setStarting(false);
    }
  };

  const again = () => {
    onView({ run: null, status: null, startedAt: 0 });
    onTask('idle');
    setProblem(null);
  };

  // No agent or no folder: say what is missing and where to get it.
  if (!agent || !project) {
    return (
      <section className="fr-step" aria-labelledby="fr-task-t">
        <div className="ck-doto fr-num" aria-hidden="true">04</div>
        <h1 className="ck-title" id="fr-task-t">{T.taskTitle}</h1>
        <p className="fr-lead">{T.taskLead}</p>
        <p className="fr-hint" role="status">{!agent ? T.taskNeedsAgent : T.taskNeedsProject}</p>
        <div className="fr-actions">
          <button type="button" className="ck-btn ck-btn--secondary" onClick={() => onGo(!agent ? 'agents' : 'project')}>
            {T.stepNames[!agent ? 'agents' : 'project']}
          </button>
        </div>
      </section>
    );
  }

  const pane = status?.pane_id ?? run?.pane_id ?? null;
  const prompt = pane ? c.prompts[pane] : null;
  const failed = settled && status.status !== 'completed';
  const explained = failed ? failedDetail(explainError(status.error?.code, agent.label)) : null;
  const lines = settled && !failed ? receiptLines(status.receipt) : [];
  const enforced = run ? run.enforced : true;

  return (
    <section className="fr-step" aria-labelledby="fr-task-t">
      <div className="ck-doto fr-num" aria-hidden="true">04</div>
      <h1 className="ck-title" id="fr-task-t">{T.taskTitle}</h1>
      <p className="fr-lead">{T.taskLead}</p>

      {!run && (
        <>
          {agents.length > 1 && (
            <div className="fr-agent-pick" role="radiogroup" aria-label={T.taskAgent}>
              {agents.map((a) => (
                <button key={a.id} type="button" role="radio" aria-checked={a.id === agent.id} className="fr-chip" onClick={() => setAgentId(a.id)}>
                  {a.label}
                </button>
              ))}
            </div>
          )}
          <h2 className="fr-cap">{T.taskAsk}</h2>
          <blockquote className="fr-ask" data-testid="task-prompt">
            {TASK_PROMPT}
          </blockquote>
          <p className="fr-hint">{T.taskSafe}</p>
          {problem && (
            <p className="fr-problem" role="alert">
              {problem}
            </p>
          )}
          <div className="fr-actions">
            <button type="button" className="ck-btn ck-btn--primary" disabled={starting} onClick={() => void start()}>
              {starting ? T.taskStarting : T.taskRun}
            </button>
          </div>
        </>
      )}

      {run && !settled && (
        <div className="fr-working" role="status">
          <DotSpinner status={prompt ? 'waiting_input' : 'running'} size={5} />
          <div>
            <p className="fr-working-line">{prompt ? T.taskAsking(agent.label) : T.taskWorking(agent.label)}</p>
            <p className="fr-hint fr-mono">{formatElapsed(Math.max(0, now - view.startedAt))}</p>
            <p className="fr-hint">{enforced ? `${T.taskSafe} ${T.taskReadOnly}` : T.taskSafeNot}</p>
          </div>
        </div>
      )}

      {run && !settled && prompt && prompt.risk && prompt.choices.length > 0 && pane && (
        <div className="fr-approval">
          <ApprovalPrompt variant="card" prompt={prompt} autoFocus={false} onAnswer={(ch, gesture) => void c.answer(pane, prompt.promptId, ch.id, ch.label, gesture)} />
          <p className="fr-tip">{T.approvalTip}</p>
        </div>
      )}

      {run && pane && !settled && (
        <div className="fr-actions">
          <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => onOpenPane(pane)}>
            {T.openPane}
          </button>
        </div>
      )}

      {failed && (
        <div className="fr-result" data-testid="task-failed">
          <div className="fr-result-head">
            <DotSpinner status="error" size={3} />
            <h2 className="ck-section">{T.taskFailed}</h2>
          </div>
          {explained && <p className="fr-hint">{explained}</p>}
          <div className="fr-actions">
            <button type="button" className="ck-btn ck-btn--secondary" onClick={again}>
              {T.taskTryAgain}
            </button>
          </div>
        </div>
      )}

      {settled && !failed && (
        <div className="fr-result" data-testid="task-done">
          <div className="fr-result-head">
            <DotSpinner status="exited" size={3} />
            <h2 className="ck-section">{T.taskAnswerTitle}</h2>
          </div>
          {status.result?.trim() ? <Answer text={status.result} /> : <p className="fr-answer">{T.taskNoAnswer}</p>}

          <div className="fr-receipt" data-testid="task-receipt">
            <h3 className="fr-cap">{T.receiptTitle}</h3>
            <dl className="fr-kv">
              {lines.map((l) => (
                <div className="fr-kv-row" key={l.key}>
                  <dt>{l.key}</dt>
                  <dd>{l.value}</dd>
                </div>
              ))}
            </dl>
          </div>
          <p className="fr-tip" data-testid="tip-receipt">{T.receiptTip}</p>
          <p className="fr-tip" data-testid="tip-undo">
            {T.undoTip} {T.undoNone}
          </p>
        </div>
      )}
    </section>
  );
}

/**
 * The Inbox's error card words, under this step's own title. The first task runs in a separate copy, so
 * the title already says the person's files were not touched; the card's "Your files are as it left
 * them" would say it twice (and less exactly), so it is left out here.
 */
export function failedDetail(e: ErrorExplanation | null): string | null {
  if (!e) return null;
  const rest = e.sentence.split(/(?<=\.)\s+/).filter((s) => !/^Your files\b/.test(s));
  return [`${e.title}.`, ...rest].join(' ');
}
