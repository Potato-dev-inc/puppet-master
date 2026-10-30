import { useCallback, useContext, useEffect, useRef, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { confirmCloseEnabled, needsCloseConfirm } from '../../lib/close-policy';
import { cockpitApi, type PaneRunInfo, type PaneTranscript } from '../../lib/cockpit-api';
import { projectLabel, projectWithCopy, realFolderPath } from './shell-labels';
import { useLoopGuard } from '../../lib/loop-guard';
import { activityText } from '../../lib/pane-activity';
import '../../styles/pane-activity.css';
import '../../styles/live-preview.css';
import '../../styles/pane-chrome.css';
import '../../styles/cockpit-helpers.css';
import { TerminalViewport } from '../TerminalViewport';
import { DotSpinner } from './DotSpinner';
import { mainPreview, openPreview, previewLabel } from '../../lib/pane-preview';
import { pointApi } from '../../lib/point-api';
import { dockItems, headerStateLine, DOCK_LABEL } from './pane-chrome';
import { IconClose, IconGlobe, IconCopy, IconInfo, IconMore, IconRail, IconReflow, IconRename, IconStop } from './Icons';
import { SessionRail } from './rail/SessionRail';
import { useRailLayout } from './rail/useRailLayout';
import { helperRows } from './rail/helperRows';
import { DockedCards } from './DockedCards';
import { HelperQuestions, helperQuestionIds } from './HelperQuestions';
import { formatElapsed, spinnerStatus, stateText } from './model';
import { ReceiptTab } from './ReceiptTab';
import { Changes } from './Changes';
import { changesTabLabel, usePaneWorktree } from './usePaneWorktree';
import { SecondOpinion } from './SecondOpinion';
import { History } from './History';
import { panesSharingFolder } from './history-model';
import { RenameTitle } from './RenameTitle';
import { useLastTurn } from './away-context';
import { SegTabs } from './SegTabs';
import { Sheet } from './Sheet';
import { paneHeading } from './titles';
import { helperOfLabel } from '../../lib/pane-helpers';
import { placeLabel, placeTooltip } from '../../lib/wsl-place';
import { isProseStream, transcriptSpeaker } from '../../lib/transcript-labels';
import { UntestedChip } from './UntestedChip';
import { BudgetChip } from './BudgetChip';
import { MissionProgress } from './mission/MissionProgress';
import { missionOfPane, useMissions } from '../../lib/missions';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import { SaveCrewSheet } from './crews/SaveCrewSheet';
import { SessionChatPanel, type ScreenAsk } from '../chat/SessionChatPanel';
import { useChatJump } from '../../lib/chatJump';
import { clearChatTabRequest, useChatTabRequest } from '../../lib/chatTab';
import { composerBlock } from '../../lib/paneOutbox';
import { LiveChip } from './live/LiveChip';
import { LiveStatusContext } from './live/useLiveRoom';

type Tab = 'mission' | 'terminal' | 'chat' | 'changes' | 'transcript' | 'receipt' | 'history';

/** Run info + transcript for the Transcript and Receipt tabs. `null` = the pane has no run. */
export function useRunData(paneId: string, active: boolean, status: string) {
  const [info, setInfo] = useState<PaneRunInfo | null | undefined>(undefined);
  const [transcript, setTranscript] = useState<PaneTranscript | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    setInfo(undefined);
    setTranscript(undefined);
    setError(null);
  }, [paneId]);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    Promise.all([cockpitApi.paneRunInfo(paneId), cockpitApi.paneTranscript(paneId)])
      .then(([i, t]) => {
        if (cancelled) return;
        setInfo(i);
        setTranscript(t);
        setError(null);
      })
      .catch((err) => !cancelled && setError(err instanceof Error ? err.message : String(err)));
    return () => {
      cancelled = true;
    };
  }, [paneId, active, status, nonce]);

  // The tab has no Refresh button: it looks again when the window gets focus back.
  useEffect(() => {
    if (!active) return;
    const onFocus = () => setNonce((n) => n + 1);
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [active]);

  // The pane reads idle a moment before its run settles (the turn end is confirmed after the screen
  // goes quiet), and the pane's status does not change again then: a fetch made at "idle" kept
  // showing "Result: Still working" for a finished turn (live r13, tour chapter 05). While the run is
  // still unsettled, look again.
  const unsettled = !!info && UNSETTLED_RUN.has(info.status);
  useEffect(() => {
    if (!active || !unsettled) return;
    const timer = setTimeout(() => setNonce((n) => n + 1), RUN_SETTLE_POLL_MS);
    return () => clearTimeout(timer);
  }, [active, unsettled, info]);

  return { info, transcript, error, refresh: () => setNonce((n) => n + 1) };
}

/** Run statuses that change without the pane's status changing. */
const UNSETTLED_RUN = new Set(['queued', 'starting', 'running', 'cancelling']);
/** How often an open Transcript or Receipt tab looks again while its run is unsettled. */
export const RUN_SETTLE_POLL_MS = 1500;

function Empty({ title, body }: { title: string; body: string }) {
  return (
    <div className="ck-empty">
      <h2 className="ck-section">{title}</h2>
      <p className="ck-sub">{body}</p>
    </div>
  );
}

export function PaneView({
  c,
  paneId,
  onWhatBroke,
}: {
  c: CockpitApi;
  paneId: string;
  /** Opens "What broke the tests?" for this pane (History tab). Absent: no button. */
  onWhatBroke?: (paneId: string) => void;
}) {
  const pane = c.registry.panes.get(paneId);
  const [tab, setTab] = useState<Tab>('terminal');
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [reflowKey, setReflowKey] = useState(0);
  const menuRef = useRef<HTMLDivElement>(null);
  const getSelectionRef = useRef<(() => string) | null>(null);
  const run = useRunData(paneId, tab !== 'terminal' && tab !== 'chat', pane?.status ?? '');
  const lastTurn = useLastTurn(paneId);
  const worktree = usePaneWorktree(paneId, pane?.status ?? '');
  const { signals } = useLoopGuard();
  const rail = useRailLayout();
  const [dockIx, setDockIx] = useState(0);
  const [details, setDetails] = useState(false);
  // "Save this team" (crews): the sheet that turns this pane and its helpers into a saved crew.
  const [saveTeam, setSaveTeam] = useState(false);
  // A pane one of a mission's agents works in gets a Mission tab: the mission's Progress.
  const missions = useMissions();
  const mission = missionOfPane(missions.missions, paneId);
  // The Chat tab reads an agent's session file, or its screen when it writes none, so a plain shell gets none (the rule TerminalPane uses).
  const listing = useAgentRegistry();
  // The Live Room status the cockpit shares; the chip shows only for a pane that is in the room.
  const liveStatus = useContext(LiveStatusContext);
  const agentType = pane?.info.agent_type ?? '';
  const chatAllowed = listing.agents.find((a) => a.id === agentType)?.kind !== 'shell';

  useEffect(() => {
    setTab('terminal');
    setMenu(false);
    setConfirm(false);
    setRenaming(false);
    setDetails(false);
    setSaveTeam(false);
    setDockIx(0);
  }, [paneId]);

  // A search result opens this pane's Chat (the chat itself scrolls to the message).
  const jump = useChatJump(paneId);
  useEffect(() => {
    if (jump && chatAllowed) setTab('chat');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jump?.nonce, paneId]);

  // Point and Fix put a message into this pane's Chat box (Edit in Chat): the Chat tab opens to show it.
  useEffect(
    () =>
      pointApi.onDraft((id) => {
        if (id === paneId && chatAllowed) setTab('chat');
      }),
    [paneId, chatAllowed],
  );

  // An old chat continued in this new pane asks for the Chat tab (lib/chatTab.ts).
  const chatTabAsk = useChatTabRequest(paneId);
  useEffect(() => {
    if (!chatTabAsk || !chatAllowed) return;
    setTab('chat');
    clearChatTabRequest(chatTabAsk);
  }, [chatTabAsk, chatAllowed]);

  useEffect(() => {
    if (tab === 'changes' && !worktree) setTab('terminal');
    if (tab === 'mission' && !mission) setTab('terminal');
    if (tab === 'chat' && !chatAllowed) setTab('terminal');
  }, [tab, worktree, mission, chatAllowed]);

  useEffect(() => {
    if (!menu) return;
    const away = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenu(false);
    };
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setMenu(false);
    window.addEventListener('mousedown', away);
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('mousedown', away);
      window.removeEventListener('keydown', esc);
    };
  }, [menu]);

  const copySelection = useCallback(async () => {
    const text = tab === 'terminal' ? (getSelectionRef.current?.() ?? '') : (window.getSelection()?.toString() ?? '');
    if (!text) {
      c.notify('Nothing selected');
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      c.notify('Copied');
    } catch {
      c.notify('Copy failed');
    }
  }, [c, tab]);

  if (!pane) {
    return (
      <div className="ck-spawn">
        <h1 className="ck-title">Opening pane</h1>
        <p className="ck-sub">Waiting for the backend to list it.</p>
      </div>
    );
  }

  const { info } = pane;
  const since = c.statusSince.get(paneId);
  const prompt = c.prompts[paneId];
  const name = c.names.get(paneId) ?? info.agent_type;
  const heading = paneHeading(info, name);
  const projectLine = [heading.secondary, projectWithCopy(info.cwd)].filter(Boolean).join(' · ');
  // A pane an agent started through Puppet Master: names its parent and opens it.
  const helperOf = helperOfLabel(c.registry.paneList, paneId, (p) => paneHeading(p.info, c.names.get(p.info.id) ?? p.info.agent_type).primary);
  const waiting = pane.status === 'waiting_input';
  const elapsed = waiting && since ? formatElapsed(c.now - since) : null;
  const stateLine = waiting && elapsed ? `Needs you · ${elapsed}` : stateText(pane.status);
  const headLine = headerStateLine({
    waiting,
    headline: prompt?.risk?.headline,
    elapsed,
    doing: activityText(pane.status, c.activities?.[paneId]),
    fallback: stateLine,
  });
  const preview = mainPreview(c.previews, paneId);

  // What waits on the person, one card at a time inside the Terminal tab; the tab strip never moves.
  const asking = waiting && !!prompt && prompt.choices.length > 0;
  const dock = dockItems(asking, !!signals[paneId]);
  const dockNow = dock.length > 0 ? dock[dockIx % dock.length] : null;
  const dockHeadline = asking ? (prompt?.risk?.headline ?? 'Question') : 'Looks stuck';
  const cycleDock = () => setDockIx((i) => i + 1);
  // A pane that waits with no card to answer on (a login or setup screen, a question no profile knows): Chat shows
  // "Agent is asking" with what the terminal asks, so the person never has to open the terminal.
  // The same card is the safety net for a pane that is only idle with a menu on its screen (see ScreenQuestionCard).
  const screenAsk: ScreenAsk = {
    waiting,
    idle: pane.status === 'idle',
    running: pane.status === 'running',
    covered: asking,
    fallbackText: prompt?.summary || prompt?.risk?.headline,
    onOpenTerminal: () => setTab('terminal'),
  };
  // The Chat tab shows the same docked cards as the Terminal tab, under the conversation.
  // The questions of this pane's helpers (its direct helpers and, for the top pane, every helper under it) wait here too.
  const helperAsks = helperQuestionIds(c, paneId);
  const chatDock =
    dockNow || helperAsks.length > 0 ? (
      <>
        <HelperQuestions c={c} ids={helperAsks} />
        {dockNow && <DockedCards c={c} paneId={paneId} dock={dock} dockNow={dockNow} onMore={cycleDock} />}
      </>
    ) : null;

  const copyTotals = worktree?.summary ?? null;

  return (
    <div className="ck-pane">
      <header className="ck-pane-head">
        <div className="ck-pane-id">
          <RenameTitle
            text={heading.primary}
            current={info.title ?? ''}
            editing={renaming}
            onEditingChange={setRenaming}
            onSave={(title) => void c.rename(paneId, title)}
          />
          <span className="ck-sub-inline" title={realFolderPath(info.cwd)}>
            {projectLine}
          </span>
          {info.account_label && <span className="ck-sub-inline" data-testid="pane-account">{info.account_label}</span>}
          {placeLabel(info.place) && (
            <span className="ck-sub-inline" data-testid="pane-place" title={placeTooltip(info.place)}>
              {placeLabel(info.place)}
            </span>
          )}
          {helperOf && (
            <button type="button" className="ck-helper-of" onClick={() => c.select(helperOf.parentId)} title="Open the pane that started this one">
              {helperOf.text}
            </button>
          )}
        </div>
        <UntestedChip agent={info.agent_type} />
        <LiveChip paneId={paneId} status={liveStatus} />
        <BudgetChip paneId={paneId} refreshKey={pane.status} onNotify={c.notify} />
        <div className="ck-pane-state">
          <DotSpinner status={spinnerStatus(pane.status)} />
          <span className="ck-pane-doing" title={stateLine}>
            {headLine}
          </span>
        </div>
        {preview && (
          <button
            type="button"
            className="ck-preview-chip"
            title={`Preview · ${preview.url.replace(/^https?:\/\//, '').replace(/\/$/, '')}. Point at something to fix it.`}
            onClick={() =>
              void pointApi.openPreview(paneId, preview.port).catch((e: unknown) => c.notify(typeof e === 'string' ? e : 'Could not open the preview'))
            }
          >
            <IconGlobe />
            <span>Preview</span>
          </button>
        )}
        {preview && (
          <button
            type="button"
            className="ck-icon-btn"
            aria-label="Open the preview in your browser"
            title="Open in your browser"
            onClick={() => void openPreview(preview.url).catch(() => c.notify('Could not open the preview'))}
          >
            <IconGlobe />
          </button>
        )}
        {rail.fits && (
          <button
            type="button"
            className="ck-icon-btn"
            aria-label={rail.open ? 'Hide session details' : 'Show session details'}
            aria-pressed={rail.open}
            title={`${rail.open ? 'Hide' : 'Show'} session details ( ] )`}
            onClick={rail.toggle}
          >
            <IconRail />
          </button>
        )}
        <div className="ck-menu-wrap" ref={menuRef}>
          <button
            type="button"
            className="ck-icon-btn"
            aria-label="Pane actions"
            aria-haspopup="menu"
            aria-expanded={menu}
            onClick={() => setMenu((v) => !v)}
          >
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
                  setRenaming(true);
                }}
              >
                <span className="ck-menu-ic"><IconRename /></span>
                <span>Rename</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  setTab('terminal');
                  setReflowKey((k) => k + 1);
                }}
              >
                <span className="ck-menu-ic"><IconReflow /></span>
                <span>Reflow</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  void copySelection();
                }}
              >
                <span className="ck-menu-ic"><IconCopy /></span>
                <span>Copy selection</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  void c.interrupt(paneId);
                }}
              >
                <span className="ck-menu-ic"><IconStop /></span>
                <span>Interrupt</span>
                <kbd className="ck-kbd">⌃C</kbd>
              </button>
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  setDetails(true);
                }}
              >
                <span className="ck-menu-ic"><IconInfo /></span>
                <span>Details</span>
              </button>
              <div className="ck-menu-sep" />
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  void confirmCloseEnabled().then((on) => {
                    if (needsCloseConfirm(pane.status, on)) setConfirm(true);
                    else void c.closePane(paneId);
                  });
                }}
              >
                <span className="ck-menu-ic"><IconClose /></span>
                <span>Close</span>
              </button>
            </div>
          )}
        </div>
      </header>

      <div className="ck-tabs-bar">
        <SegTabs
          label="Pane view"
          value={tab}
          onChange={setTab}
          options={[
            ...(mission ? [{ id: 'mission' as const, label: 'Mission' }] : []),
            { id: 'terminal', label: 'Terminal' },
            ...(chatAllowed ? [{ id: 'chat' as const, label: 'Chat' }] : []),
            ...(worktree ? [{ id: 'changes' as const, label: changesTabLabel(worktree) }] : []),
            { id: 'transcript', label: 'Transcript' },
            { id: 'receipt', label: 'Receipt' },
            { id: 'history', label: 'History' },
          ]}
        />
      </div>

      {dockNow && tab !== 'terminal' && tab !== 'chat' && (
        <button type="button" className="ck-dock-bar" onClick={() => setTab('terminal')} aria-label={`Needs you: ${dockHeadline}. Open the terminal to answer.`}>
          <span className="ck-dock-bar-dot" aria-hidden="true" />
          <span className="ck-dock-bar-text">Needs you · {dockHeadline}</span>
          <span className="ck-dock-bar-go">{DOCK_LABEL[dockNow]} ›</span>
        </button>
      )}

      <div className="ck-pane-row">
        <div className="ck-pane-body">
          {tab === 'mission' && mission && (
            <div className="ck-scroll ck-fade-in" key="mission">
              <MissionProgress mission={mission} onChanged={missions.reload} onOpenPane={(id) => c.select(id)} onNotify={c.notify} />
            </div>
          )}
          {tab === 'terminal' && (
            <div className="ck-term-wrap" key={paneId}>
              {dockNow && <DockedCards c={c} paneId={paneId} dock={dock} dockNow={dockNow} onMore={cycleDock} />}
              <div className="ck-term-slot">
                <TerminalViewport pane={pane} registry={c.registry} chrome={false}
                  redrawKey={reflowKey}
                  getSelectionRef={getSelectionRef}
                />
              </div>
            </div>
          )}
          {tab === 'chat' && chatAllowed && (
            <div className="ck-scroll ck-fade-in" key="chat">
              <SessionChatPanel paneId={paneId} dock={chatDock} screenAsk={screenAsk} reply={{ blockedReason: composerBlock(pane, !chatAllowed) }} onOpenPane={(id) => c.select(id)} />
            </div>
          )}
          {tab === 'changes' && worktree && (
            <div className="ck-scroll ck-fade-in" key="changes">
              <Changes
                paneId={paneId}
                status={pane?.status ?? ''}
                secondOpinion={<SecondOpinion paneId={paneId} status={pane?.status ?? ''} />}
              />
            </div>
          )}
          {tab === 'transcript' && (
            <div className="ck-scroll ck-fade-in" key="transcript">
              {run.error ? (
                <Empty title="Transcript unavailable" body={run.error} />
              ) : run.transcript === undefined ? (
                <p className="ck-sub ck-pad">Loading…</p>
              ) : run.transcript === null || run.transcript.chunks.length === 0 ? (
                <Empty
                  title="No transcript"
                  body={
                    run.info === null
                      ? 'A transcript is kept for tasks that another agent handed to this one. What you typed here yourself is on the Terminal tab.'
                      : 'The run has not produced any output yet.'
                  }
                />
              ) : (
                <div className="ck-transcript">
                  {run.transcript.chunks.map((chunk, i) => (
                    <div key={i} className={`ck-chunk${isProseStream(chunk.stream) ? ' ck-chunk--prose' : ''}`}>
                      <span className="ck-chunk-stream">{transcriptSpeaker(chunk.stream, name)}</span>
                      <pre>{chunk.text}</pre>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
          {tab === 'history' && (
            <History
              paneId={paneId}
              status={pane.status}
              now={c.now}
              notify={c.notify}
              agentName={name}
              sharedWith={panesSharingFolder(c.registry.paneList, c.names, paneId)}
              onWhatBroke={onWhatBroke}
            />
          )}
          {tab === 'receipt' && (
            <div className="ck-scroll ck-fade-in" key="receipt">
              <ReceiptTab
                info={run.info}
                error={run.error}
                lastTurn={lastTurn}
                now={c.now}
                copy={copyTotals}
                agentName={name}
                // An isolated pane has its own Changes tab, which holds the second opinion.
                secondOpinion={!worktree ? <SecondOpinion paneId={paneId} status={pane?.status ?? ''} /> : null}
              />
            </div>
          )}
        </div>
        {rail.visible && (
          <SessionRail
            paneId={paneId}
            status={pane.status}
            cwd={info.cwd}
            agentName={name}
            accountLabel={info.account_label}
            readsChat={chatAllowed}
            helperOf={helperOf}
            helpers={helperRows(c.registry.paneList, paneId, (p) => c.names.get(p.info.id) ?? p.info.agent_type)}
            onOpenChanges={worktree ? () => setTab('changes') : undefined}
            onOpenPane={(id) => c.select(id)}
            onSaveTeam={() => setSaveTeam(true)}
          />
        )}
      </div>

      {saveTeam && (
        <SaveCrewSheet
          paneId={paneId}
          agentName={(id) => agentLabel(listing, id)}
          onClose={() => setSaveTeam(false)}
          onSaved={(crewName) => {
            setSaveTeam(false);
            c.notify(`Saved the crew ${crewName}`);
          }}
        />
      )}

      {details && (
        <Sheet labelledBy="ck-details-t" onClose={() => setDetails(false)}>
          <h2 className="ck-section" id="ck-details-t">
            Details
          </h2>
          <dl className="ck-facts ck-facts--details">
            <div className="ck-facts-row">
              <dt>Folder</dt>
              <dd className="ck-mono">{info.cwd}</dd>
            </div>
            {worktree && (
              <div className="ck-facts-row">
                <dt>Separate copy</dt>
                <dd>
                  <span className="ck-mono">{worktree.branch}</span> from <span className="ck-mono">{worktree.base}</span> in {projectLabel(worktree.repo)}
                </dd>
              </div>
            )}
            <div className="ck-facts-row">
              <dt>Screen size</dt>
              <dd className="ck-mono">
                {info.cols}×{info.rows}
              </dd>
            </div>
            <div className="ck-facts-row">
              <dt>Process ID</dt>
              <dd className="ck-mono">{info.pid}</dd>
            </div>
          </dl>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={() => setDetails(false)}>
              Done
            </button>
          </div>
        </Sheet>
      )}

      {confirm && (
        <Sheet labelledBy="ck-close-t" onClose={() => setConfirm(false)}>
            <h2 className="ck-section" id="ck-close-t">
              Close {name}?
            </h2>
            <p className="ck-sub">It is still running. {name} stops, and its conversation ends.</p>
            <div className="ck-sheet-actions">
              <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={() => setConfirm(false)}>
                Keep running
              </button>
              <button
                type="button"
                className="ck-btn ck-btn--primary"
                onClick={() => {
                  setConfirm(false);
                  void c.closePane(paneId);
                }}
              >
                Close agent
              </button>
            </div>
        </Sheet>
      )}
    </div>
  );
}
