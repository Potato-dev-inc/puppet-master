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
import { problemForCode, problemForLoop, type Problem } from './error-text';
import { needsScreen } from './screen-model';
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
  api?: Pick<SimpleApi, 'preview' | 'openPreview' | 'start'>;
}) {
  const pane = c.registry.panes.get(paneId);
  const status = pane?.status ?? '';
  const listing = useAgentRegistry();
  const agent = pane ? agentLabel(listing, pane.info.agent_type) : '';
  const copy = useCopy(paneId, status);
  const run = useRunData(paneId, true, status);
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
  const [queued, setQueued] = useState<string | null>(null);
  const answered = useRef(new Set<string>());
  const getSelectionRef = useRef<(() => string) | null>(null);

  useEffect(() => {
    setTab('preview');
    setMenu(false);
    setScreenOpen(false);
    setBlocked(null);
    setQueued(null);
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

  // A message typed while the agent works waits and is sent when it is ready again.
  useEffect(() => {
    if (queued && status === 'idle') {
      void c.registry.writeInput(paneId, queued, true).catch(() => c.notify(T.somethingWrong));
      setQueued(null);
    }
  }, [queued, status, c, paneId]);

  if (!pane) return <div className="sx-page"><p className="sx-quiet">{T.thinking}</p></div>;

  const working = status === 'running';
  const item = copy.item;
  const name = projectLabel(item, agent);
  const files = item?.summary?.files_changed ?? 0;
  const loopSignal = loop.signals[paneId];
  const problem: Problem | null =
    status === 'error'
      ? problemForCode(run.info?.error?.code, agent)
      : loopSignal
        ? problemForLoop(loopSignal, agent)
        : null;
  const forcedScreen = needsScreen({ status, prompt }) || (blocked !== null && !blocked.stoppable);
  const showScreen = screenOpen || forcedScreen;
  const ask = status === 'waiting_input' && prompt && prompt.risk && prompt.risk.level !== 'high' && prompt.choices.length > 0 ? prompt : null;

  const answer = (choice: PromptChoice) => {
    if (!prompt) return;
    cockpitApi.answerPanePrompt(paneId, prompt.promptId, choice.id).catch(() => c.notify(T.somethingWrong));
  };

  const send = () => {
    const text = draft.trim();
    if (!text) return;
    setDraft('');
    if (status === 'idle') void c.registry.writeInput(paneId, text, true).catch(() => c.notify(T.somethingWrong));
    else setQueued(text);
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
      c.notify(/CONFLICT|OVERLAP|DIRTY/.test(text) ? T.keepConflict : T.keepFailed);
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
            <Changes paneId={paneId} status={status} plain hideActions />
          </div>
        )}
        {tab === 'goback' && <GoBackTab paneId={paneId} status={status} now={c.now} agent={agent} notify={c.notify} />}
      </div>

      {showScreen && (
        <section className="sx-screen" aria-label={T.agentScreen}>
          <p className="sx-quiet">{forcedScreen ? F.cantRead(agent) : F.agentScreenCaption(agent)}</p>
          <div className="sx-screen-term">
            <TerminalViewport pane={pane} registry={c.registry} chrome={false} getSelectionRef={getSelectionRef} />
          </div>
        </section>
      )}

      <div className="sx-composer">
        {queued && <p className="sx-quiet">{F.willSendWhenDone(agent)}</p>}
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
          <button type="button" className="ck-btn ck-btn--secondary" disabled={!draft.trim()} onClick={send}>
            {T.composerSend}
          </button>
        </div>
      </div>

      <footer className="sx-bar">
        <span className="sx-quiet">{working ? T.keepWorking : files === 0 ? T.keepNothing : F.changedSummary(agent, files)}</span>
        <span className="sx-bar-actions">
          {working && (
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void c.interrupt(paneId)}>
              {T.stop}
            </button>
          )}
          <button type="button" className="ck-btn ck-btn--plain" disabled={!item || busy || working} onClick={() => setSheet('throw')}>
            {T.throwAway}
          </button>
          <button type="button" className="ck-btn ck-btn--primary" disabled={!item || busy || working || files === 0} onClick={() => setSheet('keep')}>
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
