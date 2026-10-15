import { useCallback, useEffect, useRef, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { confirmCloseEnabled, needsCloseConfirm } from '../../lib/close-policy';
import { cockpitApi, type PaneRunInfo, type PaneTranscript } from '../../lib/cockpit-api';
import { projectName } from '../../lib/mobile-agent-tabs';
import { activityText } from '../../lib/pane-activity';
import '../../styles/pane-activity.css';
import '../../styles/live-preview.css';
import { PaneStuckCard } from './StuckCard';
import { TerminalViewport } from '../TerminalViewport';
import { DotSpinner } from './DotSpinner';
import { mainPreview, openPreview, previewLabel } from '../../lib/pane-preview';
import { IconClose, IconGlobe, IconCopy, IconMore, IconReflow, IconRename, IconStop } from './Icons';
import { ApprovalPrompt } from './ApprovalPrompt';
import { formatElapsed, promptSummary, spinnerStatus, stateText } from './model';
import { readableReceipt } from './receipt-view';
import { Changes } from './Changes';
import { changesTabLabel, usePaneWorktree } from './usePaneWorktree';
import { SecondOpinion } from './SecondOpinion';
import { History } from './History';
import { panesSharingFolder } from './history-model';
import { RenameTitle } from './RenameTitle';
import { LastTurn } from './LastTurn';
import { useLastTurn } from './away-context';
import { SegTabs } from './SegTabs';
import { Sheet } from './Sheet';
import { paneHeading } from './titles';
import { transcriptStreamLabel } from '../../lib/transcript-labels';
import { UntestedChip } from './UntestedChip';
import { BudgetChip } from './BudgetChip';
import { MissionProgress } from './mission/MissionProgress';
import { missionOfPane, useMissions } from '../../lib/missions';

type Tab = 'mission' | 'terminal' | 'changes' | 'transcript' | 'receipt' | 'history';

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

export function PaneView({ c, paneId }: { c: CockpitApi; paneId: string }) {
  const pane = c.registry.panes.get(paneId);
  const [tab, setTab] = useState<Tab>('terminal');
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [reflowKey, setReflowKey] = useState(0);
  const menuRef = useRef<HTMLDivElement>(null);
  const getSelectionRef = useRef<(() => string) | null>(null);
  const run = useRunData(paneId, tab !== 'terminal', pane?.status ?? '');
  const lastTurn = useLastTurn(paneId);
  const worktree = usePaneWorktree(paneId, pane?.status ?? '');
  // A pane one of a mission's agents works in gets a Mission tab: the mission's Progress.
  const missions = useMissions();
  const mission = missionOfPane(missions.missions, paneId);

  useEffect(() => {
    setTab('terminal');
    setMenu(false);
    setConfirm(false);
    setRenaming(false);
  }, [paneId]);

  useEffect(() => {
    if (tab === 'changes' && !worktree) setTab('terminal');
    if (tab === 'mission' && !mission) setTab('terminal');
  }, [tab, worktree, mission]);

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
  const stateLine =
    pane.status === 'waiting_input' && since ? `Needs you · ${formatElapsed(c.now - since)}` : stateText(pane.status);
  const doing = activityText(pane.status, c.activities?.[paneId]);
  const headLine = doing
    ? pane.status === 'waiting_input' && since
      ? `${doing} · ${formatElapsed(c.now - since)}`
      : doing
    : stateLine;

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
          <span className="ck-sub-inline">
            {heading.secondary ? `${heading.secondary} · ${projectName(info.cwd)}` : projectName(info.cwd)}
          </span>
          {info.account_label && <span className="ck-sub-inline" data-testid="pane-account">{info.account_label}</span>}
        </div>
        <UntestedChip agent={info.agent_type} />
        <BudgetChip paneId={paneId} refreshKey={pane.status} onNotify={c.notify} />
        <div className="ck-pane-state">
          <DotSpinner status={spinnerStatus(pane.status)} />
          <span className="ck-pane-doing" title={stateLine}>
            {headLine}
          </span>
        </div>
        {mainPreview(c.previews, paneId) && (
          <button
            type="button"
            className="ck-preview-chip"
            title={`Open ${mainPreview(c.previews, paneId)!.url} in your browser`}
            onClick={() => void openPreview(mainPreview(c.previews, paneId)!.url).catch(() => c.notify('Could not open the preview'))}
          >
            <IconGlobe />
            <span>Preview</span>
            <span className="ck-preview-port">{previewLabel(mainPreview(c.previews, paneId)!)}</span>
          </button>
        )}
        <span className="ck-grid-size">
          {info.cols}×{info.rows}
        </span>
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

      <PaneStuckCard c={c} paneId={paneId} />

      {pane.status === 'waiting_input' && prompt && prompt.choices.length > 0 && (
        <div className="ck-ask" role="group" aria-label="Agent question">
          {prompt.risk && <ApprovalPrompt variant="bar" prompt={prompt} onAnswer={(ch) => void c.answer(paneId, prompt.promptId, ch.id, ch.label)} />}
          {!prompt.risk && <span className="ck-ask-q">{promptSummary(prompt)}</span>}
          <span className="ck-ask-actions">
            {!prompt.risk && prompt.choices.map((ch, i) => (
              <button
                key={ch.id}
                type="button"
                className={`ck-btn ck-btn--sm ${i === 0 ? 'ck-btn--primary' : 'ck-btn--secondary'}`}
                onClick={() => void c.answer(paneId, prompt.promptId, ch.id, ch.label)}
              >
                {ch.label}
              </button>
            ))}
          </span>
        </div>
      )}

      <div className="ck-tabs-bar">
        <SegTabs
          label="Pane view"
          value={tab}
          onChange={setTab}
          options={[
            ...(mission ? [{ id: 'mission' as const, label: 'Mission' }] : []),
            { id: 'terminal', label: 'Terminal' },
            ...(worktree ? [{ id: 'changes' as const, label: changesTabLabel(worktree) }] : []),
            { id: 'transcript', label: 'Transcript' },
            { id: 'receipt', label: 'Receipt' },
            { id: 'history', label: 'History' },
          ]}
        />
      </div>

      <div className="ck-pane-body">
        {tab === 'mission' && mission && (
          <div className="ck-scroll ck-fade-in" key="mission">
            <MissionProgress mission={mission} onChanged={missions.reload} onOpenPane={(id) => c.select(id)} onNotify={c.notify} />
          </div>
        )}
        {tab === 'terminal' && (
          <div className="ck-term-slot" key={paneId}>
            <TerminalViewport pane={pane} registry={c.registry} chrome={false}
              reflowKey={reflowKey}
              getSelectionRef={getSelectionRef}
            />
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
                  <div key={i} className="ck-chunk">
                    <span className="ck-chunk-stream">{transcriptStreamLabel(chunk.stream)}</span>
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
            sharedWith={panesSharingFolder(c.registry.paneList, c.names, paneId)}
          />
        )}
        {tab === 'receipt' && (
          <div className="ck-scroll ck-fade-in" key="receipt">
            {/* An isolated pane has its own Changes tab, which holds the second opinion. */}
            {!worktree && <SecondOpinion paneId={paneId} status={pane?.status ?? ''} />}
            <LastTurn summary={lastTurn} now={c.now} />
            {run.error ? (
              <Empty title="Receipt unavailable" body={run.error} />
            ) : run.info === undefined ? (
              <p className="ck-sub ck-pad">Loading…</p>
            ) : run.info === null ? (
              <Empty
                title="No receipt for this pane"
                body="A receipt is written for tasks that another agent handed to this one, and says what was checked. Work you started here yourself has none."
              />
            ) : (
              <div className="ck-receipt">
                {run.info.receipt === null || run.info.receipt === undefined ? (
                  <p className="ck-sub">No receipt yet. It is written when the turn settles.</p>
                ) : (
                  <dl className="ck-kv">
                    {readableReceipt(run.info.receipt).map((r) => (
                      <div className="ck-kv-row" key={r.key}>
                        <dt>{r.key}</dt>
                        <dd>{r.value}</dd>
                      </div>
                    ))}
                  </dl>
                )}
                <details className="ck-details">
                  <summary>Details</summary>
                  <dl className="ck-kv">
                    <dt>Run ID</dt>
                    <dd className="ck-mono">{run.info.handle}</dd>
                  </dl>
                </details>
              </div>
            )}
            <button type="button" className="ck-btn ck-btn--plain ck-refresh" onClick={run.refresh}>
              Refresh
            </button>
          </div>
        )}
      </div>

      {confirm && (
        <Sheet labelledBy="ck-close-t" onClose={() => setConfirm(false)}>
            <h2 className="ck-section" id="ck-close-t">
              Close {name}?
            </h2>
            <p className="ck-sub">It is still running. Closing ends the process and its session.</p>
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
                Close pane
              </button>
            </div>
        </Sheet>
      )}
    </div>
  );
}
