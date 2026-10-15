// One project in Simple mode: Preview, Conversation, What changed, Go back. It works on a COPY, so
// the bottom bar offers Keep (put the changes in her own folder) and Throw away.
//
// Nothing here forks the cockpit's logic: the run data is `useRunData` (the Transcript tab's),
// What changed is the `Changes` component in plain mode, Go back restores through `restoreWithUndo`
// (the History tab's), and the agent's screen is the same `TerminalViewport`.
import { useEffect, useRef, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import { cockpitApi, type PromptChoice } from '../../lib/cockpit-api';
import { useLoopGuard } from '../../lib/loop-guard';
import { Changes } from '../cockpit/Changes';
import { DotSpinner } from '../cockpit/DotSpinner';
import { IconMore } from '../cockpit/Icons';
import { SegTabs } from '../cockpit/SegTabs';
import { Sheet } from '../cockpit/Sheet';
import { spinnerStatus } from '../cockpit/model';
import { useRunData } from '../cockpit/PaneView';
import { useLastTurn } from '../cockpit/away-context';
import { TerminalViewport } from '../TerminalViewport';
import { simpleApproval, blockChoice } from './approval-text';
import { AskCard, BlockedCard, ProblemCard } from './Cards';
import { ConversationTab } from './ConversationTab';
import { GoBackTab } from './GoBackTab';
import { PreviewTab } from './PreviewTab';
import { F, T } from './copy';
import { currentProblem } from './error-text';
import { endState, needsScreen, shouldResendTask } from './screen-model';
import { errorCode, simpleApi, type SimpleApi } from './simple-api';
import { projectLabel, useCopy } from './use-copy';

type Tab = 'preview' | 'conversation' | 'changed' | 'goback';

function stateWords(status: string): string {
  switch (status) {
    case 'running':
      return T.stateWorking;
    case 'waiting_input':
      return T.stateNeedsYou;
    case 'idle':
      return T.stateReady;
    default:
      return T.stateStopped;
  }
}

/** True from 1200px up: Preview and Conversation sit side by side. */
function useWide(): boolean {
  const query = '(min-width: 1200px)';
  const [wide, setWide] = useState(() => typeof window !== 'undefined' && window.matchMedia?.(query).matches === true);
  useEffect(() => {
    const mq = window.matchMedia?.(query);
    if (!mq) return;
    const on = () => setWide(mq.matches);
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  }, []);
  return wide;
}

export function SimpleProject({
  c,
  paneId,
  onBack,
  onStartedAgain,
  api = simpleApi,
}: {
  c: CockpitApi;
  paneId: string;
  onBack: () => void;
  onStartedAgain: (paneId: string | null) => void;
  api?: Pick<SimpleApi, 'preview' | 'openPreview' | 'start' | 'send'>;
}) {
  const pane = c.registry.panes.get(paneId);
  const status = pane?.status ?? '';
  const listing = useAgentRegistry();
  const agent = pane ? agentLabel(listing, pane.info.agent_type) : '';
  const run = useRunData(paneId, true, status);
  // The copy's totals settle a moment after the agent goes idle: look again when the run's state changes too.
  const settle = `${status}|${run.info?.status ?? ''}`;
  const copy = useCopy(paneId, settle);
  const lastTurn = useLastTurn(paneId);
  const loop = useLoopGuard();
  const prompt = c.prompts[paneId];
  const wide = useWide();

  const [tab, setTab] = useState<Tab>('preview');
  const [menu, setMenu] = useState(false);
  const [screenOpen, setScreenOpen] = useState(false);
  const [blocked, setBlocked] = useState<{ what: string; stoppable: boolean } | null>(null);
  const [sheet, setSheet] = useState<'keep' | 'throw' | null>(null);
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState('');
  const [queuedNote, setQueuedNote] = useState(false);
  const answered = useRef(new Set<string>());
  const getSelectionRef = useRef<(() => string) | null>(null);

  useEffect(() => {
    setTab('preview');
    setMenu(false);
    setScreenOpen(false);
    setBlocked(null);
    setQueuedNote(false);
  }, [paneId]);

  // High risk is never asked. PM answers the agent's own No, then tells her (a blocked card).
  useEffect(() => {
    if (!pane || status !== 'waiting_input' || !prompt || prompt.risk?.level !== 'high') return;
    if (answered.current.has(prompt.promptId)) return;
    answered.current.add(prompt.promptId);
    const words = simpleApproval(prompt);
    const choice = blockChoice(prompt);
    if (!choice) {
      setBlocked({ what: words?.what ?? '', stoppable: false });
      return;
    }
    cockpitApi
      .answerPanePrompt(paneId, prompt.promptId, choice.id)
      .then(() => setBlocked({ what: words?.what ?? '', stoppable: true }))
      .catch(() => setBlocked({ what: words?.what ?? '', stoppable: false }));
  }, [pane, paneId, prompt, status]);

  // While the agent works, new words arrive in the transcript: look again every few seconds.
  const refreshRun = useRef(run.refresh);
  refreshRun.current = run.refresh;
  useEffect(() => {
    // Not only while it works: the run settles a moment after the agent is idle.
    const timer = window.setInterval(() => refreshRun.current(), status === 'running' ? 3000 : 6000);
    return () => window.clearInterval(timer);
  }, [status]);

  // The folder-trust question parks the run before anything is typed. When she has answered it
  // (the agent is idle again), send the same task once more: she already pressed Start on these words.
  const resent = useRef(new Set<string>());
  const firstTask = run.transcript?.chunks.find((chunk) => chunk.stream === 'user_task')?.text;
  useEffect(() => {
    const handle = run.info?.handle;
    if (!copy.item || !handle || !firstTask) return;
    const errorCode = run.info?.error?.code;
    if (!shouldResendTask({ status, errorCode, handle, task: firstTask, alreadySent: resent.current.has(handle) })) return;
    resent.current.add(handle);
    api
      .send(copy.item.repo, handle, firstTask)
      .then(() => refreshRun.current())
      .catch(() => c.notify(T.somethingWrong));
  }, [api, c, copy.item, firstTask, run.info, status]);

  if (!pane) return <div className="sx-page"><p className="sx-quiet">{T.thinking}</p></div>;

  const working = status === 'running';
  const item = copy.item;
  const name = projectLabel(item, agent);
  const files = item?.summary?.files_changed ?? 0;
  const end = endState({ status, copyState: item?.state, files });
  const endBusy = end === 'working' || end === 'waiting';
  const loopSignal = loop.signals[paneId];
  const problem = currentProblem({
    status,
    runStatus: run.info?.status,
    runCode: run.info?.error?.code,
    loop: loopSignal,
    agent,
  });
  // Something PM cannot word as a card: say so, and keep the raw screen behind one button. A stop that
  // cannot be told in words (blocked, not stoppable) still shows the screen at once.
  const cantRead = needsScreen({ status, prompt });
  const forcedScreen = blocked !== null && !blocked.stoppable;
  const showScreen = screenOpen || forcedScreen;
  const ask = status === 'waiting_input' && prompt && prompt.risk && prompt.risk.level !== 'high' && prompt.choices.length > 0 ? prompt : null;

  const answer = (choice: PromptChoice) => {
    if (!prompt) return;
    cockpitApi.answerPanePrompt(paneId, prompt.promptId, choice.id).catch(() => c.notify(T.somethingWrong));
  };

  const send = () => {
    const text = draft.trim();
    const handle = run.info?.handle;
    if (!text || !item || !handle) return;
    setDraft('');
    // The run queues a message that arrives while the agent works, and sends it when it finishes.
    setQueuedNote(working);
    api.send(item.repo, handle, text).then(
      () => run.refresh(),
      () => c.notify(T.somethingWrong),
    );
  };

  const finish = async (action: 'keep' | 'throw') => {
    if (!item) return;
    setBusy(true);
    try {
      if (action === 'keep') {
        const out = await cockpitApi.worktreeMerge(item.repo, item.handle);
        c.notify(out.merged ? T.keepDone : T.keepNothing);
      } else {
        await cockpitApi.worktreeDiscard(item.repo, item.handle);
        c.notify(T.throwDone);
      }
      setSheet(null);
      onBack();
    } catch (err) {
      setSheet(null);
      const text = err instanceof Error ? err.message : String(err);
      c.notify(
        /RUN_ACTIVE/.test(text) ? T.keepWorking : /CONFLICT|OVERLAP|DIRTY/.test(text) ? T.keepConflict : action === 'throw' ? T.throwFailed : T.keepFailed,
      );
    } finally {
      setBusy(false);
    }
  };

  const startAgain = async () => {
    if (!item) return;
    const task = run.transcript?.chunks.find((chunk) => chunk.stream === 'user_task')?.text;
    if (!task) return;
    try {
      const started = await api.start({ name, prompt: task, agent: pane.info.agent_type, existing_folder: item.repo });
      onStartedAgain(started.pane_id);
    } catch (err) {
      c.notify(errorCode(err) ? T.makeFailed : T.somethingWrong);
    }
  };

  const tellWhy = () => {
    void c.registry.writeInput(paneId, T.blockedTellText, true).then(
      () => c.notify(T.blockedTold),
      () => c.notify(T.somethingWrong),
    );
  };

  const convo = (
    <ConversationTab transcript={run.transcript} error={run.error} lastTurn={lastTurn} agent={agent} working={working} />
  );

  return (
    <div className="sx-project">
      <header className="sx-head">
        <button type="button" className="sx-back" onClick={onBack}>
          ‹ {T.back}
        </button>
        <div className="sx-head-title">
          <h1 className="ck-title ck-title--sm">{name}</h1>
          <span className="sx-quiet" title={T.copyNote}>
            {copy.loaded && !item ? T.notOnCopy : F.workingOnCopy(agent)}
          </span>
        </div>
        <div className="sx-head-state" aria-live="polite">
          <span aria-hidden="true">
            <DotSpinner status={spinnerStatus(status)} />
          </span>
          <span>{stateWords(status)}</span>
        </div>
        <div className="ck-menu-wrap">
          <button type="button" className="ck-icon-btn" aria-label={T.more} aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu((m) => !m)}>
            <IconMore />
          </button>
          {menu && (
            <div className="ck-menu" role="menu">
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  setScreenOpen((open) => !open);
                }}
              >
                <span>
                  {T.details} · {showScreen && screenOpen ? T.hideScreen : T.agentScreen}
                </span>
              </button>
            </div>
          )}
        </div>
      </header>

      {ask && <AskCard agent={agent} prompt={ask} onAnswer={answer} />}
      {blocked && (
        <BlockedCard
          agent={agent}
          what={blocked.what}
          onOk={() => setBlocked(null)}
          onTell={() => {
            tellWhy();
            setBlocked(null);
          }}
        />
      )}
      {cantRead && !showScreen && (
        <section className="sx-card" role="group" aria-label={F.cantRead(agent)} data-testid="cant-read">
          <span className="sx-card-glyph" aria-hidden="true">
            <DotSpinner status="waiting_input" />
          </span>
          <div className="sx-card-body">
            <p className="sx-card-text">{F.cantRead(agent)}</p>
            <p className="sx-card-tag">{T.cantReadHint}</p>
          </div>
          <div className="sx-card-actions">
            <button type="button" className="ck-btn ck-btn--primary" onClick={() => setScreenOpen(true)}>
              {T.showScreen}
            </button>
          </div>
        </section>
      )}
      {problem && <ProblemCard problem={problem} onStartAgain={() => void startAgain()} onShowMe={() => setScreenOpen(true)} />}

      <div className="sx-tabs">
        <SegTabs
          label={T.projects}
          value={tab}
          onChange={setTab}
          options={[
            { id: 'preview', label: T.tabPreview },
            { id: 'conversation', label: T.tabConversation },
            { id: 'changed', label: T.tabChanged },
            { id: 'goback', label: T.tabGoBack },
          ]}
        />
      </div>

      <div className={`sx-body${showScreen ? ' has-screen' : ''}`}>
        {tab === 'preview' && (
          <div className={wide ? 'sx-split' : 'sx-fill'}>
            <div className="sx-fill">
              <PreviewTab paneId={paneId} status={status} agent={agent} api={api} onNotify={c.notify} />
            </div>
            {wide && <div className="sx-side sx-scroll">{convo}</div>}
          </div>
        )}
        {tab === 'conversation' && <div className="sx-scroll">{convo}</div>}
        {tab === 'changed' && (
          <div className="sx-scroll">
            <Changes paneId={paneId} status={settle} plain hideActions />
          </div>
        )}
        {tab === 'goback' && <GoBackTab paneId={paneId} status={status} now={c.now} agent={agent} notify={c.notify} />}
      </div>

      {showScreen && (
        <section className="sx-screen" aria-label={T.agentScreen}>
          <p className="sx-quiet">{F.agentScreenCaption(agent)}</p>
          <div className="sx-screen-term">
            <TerminalViewport pane={pane} registry={c.registry} chrome={false} getSelectionRef={getSelectionRef} />
          </div>
        </section>
      )}

      <div className="sx-composer">
        {queuedNote && working && <p className="sx-quiet">{F.willSendWhenDone(agent)}</p>}
        <div className="sx-composer-row">
          <textarea
            className="sx-input sx-composer-input"
            rows={1}
            aria-label={F.tellAgent(agent)}
            placeholder={F.tellAgent(agent)}
            value={draft}
            disabled={status === 'error'}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
          />
          <button type="button" className="ck-btn ck-btn--secondary" disabled={!draft.trim() || !run.info?.handle} onClick={send}>
            {T.composerSend}
          </button>
        </div>
      </div>

      <footer className="sx-bar">
        <span className="sx-quiet">
          {end === 'working' ? T.keepWorking : end === 'waiting' ? T.keepWaiting : end === 'empty' ? T.keepNothing : F.changedSummary(agent, files)}
        </span>
        <span className="sx-bar-actions">
          {working && (
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void c.interrupt(paneId)}>
              {T.stop}
            </button>
          )}
          <button type="button" className="ck-btn ck-btn--plain" disabled={!item || busy || endBusy} onClick={() => setSheet('throw')}>
            {T.throwAway}
          </button>
          <button type="button" className="ck-btn ck-btn--primary" disabled={!item || busy || endBusy || files === 0} onClick={() => setSheet('keep')}>
            {T.keep}
          </button>
        </span>
      </footer>

      {sheet && (
        <Sheet labelledBy="sx-end-t" onClose={busy ? undefined : () => setSheet(null)}>
          <h2 className="ck-section" id="sx-end-t">
            {sheet === 'keep' ? T.keepTitle : T.throwTitle}
          </h2>
          <p className="ck-sub">{sheet === 'keep' ? F.keepBody(files) : T.throwBody}</p>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={() => setSheet(null)}>
              {T.cancel}
            </button>
            <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void finish(sheet)}>
              {sheet === 'keep' ? T.keepConfirm : T.throwConfirm}
            </button>
          </div>
        </Sheet>
      )}
    </div>
  );
}
