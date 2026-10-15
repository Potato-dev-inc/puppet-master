// "All" tab of the mobile PWA: a large "Agents" title with the number waiting for you (Doto), then
// the agent panes grouped Needs you / Working / Idle as an inset list. Each row shows the state glyph,
// the pane's title, its project and last line, and how long ago it printed. Tapping opens that tab.
import { useEffect, useState, type ReactNode } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import type { BridgeClient } from '../../lib/bridge';
import type { LoopSignalInfo } from '../../lib/bridge-types';
import { ago, glyphStatus, groupPanes, previewLines, projectName, waitingCount } from '../../lib/mobile-agent-tabs';
import { DotSpinner } from '../cockpit/DotSpinner';
import { DigestCard } from './DigestCard';
import { isLive } from '../../lib/questions';
import { blockingCount, type AttentionSnapshot } from '../../lib/attention';
import { QuestionsView, useQuestionPolling } from './QuestionsList';
import { NeedsYouList } from './NeedsYouList';
import { TeamGroup } from './TeamGroup';

const PREVIEW_POLL_MS = 4000;

function Chevron() {
  return (
    <span className="ph-chev" aria-hidden="true">
      <svg viewBox="0 0 24 24">
        <path d="m9 5 7 7-7 7" />
      </svg>
    </span>
  );
}

export function AgentsOverview({
  bridge,
  bridgeReady,
  panes,
  labels,
  onOpen,
  onNew,
  header,
  stuck,
  questionsRevision,
  attention,
  canAnswer = true,
  onRefreshAttention,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  panes: PaneInfo[];
  labels: Map<string, string>;
  onOpen: (paneId: string) => void;
  onNew: () => void;
  /** Rendered above the list (pending agent questions). */
  header?: ReactNode;
  /** Panes the loop guard flagged as stuck, by pane id. */
  stuck?: Map<string, LoopSignalInfo>;
  /**
   * Bumped by the bridge `questions` event. When given, this overview fetches the agents' questions,
   * shows their cards under the title, and counts them in the waiting number like the desktop does.
   */
  questionsRevision?: number;
  /**
   * The PC's one attention snapshot. When it is there, the waiting number IS its blocking count (the
   * number the desktop Inbox, the tray and the away digest show); an old PC leaves it out and this
   * screen counts what it can see by itself.
   */
  attention?: AttentionSnapshot | null;
  /** This phone holds the `answer` ability (approval cards without it have no buttons). */
  canAnswer?: boolean;
  /** An answer came back stale: read the PC's list again now. */
  onRefreshAttention?: () => void;
}) {
  const { questions, refresh } = useQuestionPolling(bridge, bridgeReady && questionsRevision !== undefined, questionsRevision ?? 0);
  const liveQuestions = questionsRevision === undefined ? [] : questions.filter((q) => isLive(q, Date.now()));
  const [previews, setPreviews] = useState<Record<string, string[]>>({});
  const [now, setNow] = useState(() => Date.now());
  const ids = panes.map((pane) => pane.id).join(',');

  useEffect(() => {
    if (!bridgeReady || !ids) return;
    let live = true;
    const tick = async () => {
      setNow(Date.now());
      const entries = await Promise.all(
        ids.split(',').map(async (id) => {
          try {
            return [id, previewLines(await bridge.readBuffer(id, 60))] as const;
          } catch {
            return [id, null] as const;
          }
        }),
      );
      if (!live) return;
      setPreviews((prev) => {
        const next: Record<string, string[]> = {};
        for (const [id, lines] of entries) next[id] = lines ?? prev[id] ?? [];
        return next;
      });
    };
    void tick();
    const timer = window.setInterval(() => void tick(), PREVIEW_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [bridge, bridgeReady, ids]);

  if (!bridgeReady) {
    return <div className="ph-connecting">Connecting to your PC…</div>;
  }

  const waiting = attention
    ? String(blockingCount(attention)).padStart(2, '0')
    : waitingCount(panes, { questions: liveQuestions, stuckPaneIds: stuck ? [...stuck.keys()] : [] });
  // With the PC's list the cards ARE that list, in its order; an older PC keeps the question cards alone.
  const cards = attention ? (
    <NeedsYouList
      snapshot={attention}
      bridge={bridge}
      canAnswer={canAnswer}
      questions={liveQuestions}
      refreshQuestions={refresh}
      labels={labels}
      onOpenPane={onOpen}
      onRefresh={onRefreshAttention}
    />
  ) : questionsRevision === undefined ? null : (
    <QuestionsView bridge={bridge} questions={questions} refresh={refresh} labels={labels} />
  );
  const title = (
    <header className="ph-large">
      <h1>Agents</h1>
      <div className="ph-counter" aria-label={`${Number(waiting)} waiting`}>
        <span className="ph-doto">{waiting}</span>
        <span className="ph-counter-label">waiting</span>
      </div>
    </header>
  );

  if (panes.length === 0) {
    return (
      <div className="ph-scroll">
        {title}
        {header}
        {cards}
        <TeamGroup bridge={bridge} bridgeReady={bridgeReady} />
        <DigestCard bridge={bridge} bridgeReady={bridgeReady} onOpen={onOpen} />
        <div className="ph-empty">
          <div className="ph-dotgrid" aria-hidden="true" />
          <DotSpinner status="idle" size={5} />
          <b>No agents running</b>
          <p>Agents you open on the PC, or that another agent starts through Puppet Master, show up here.</p>
          <button type="button" onClick={onNew} className="ph-btn ph-btn--primary">
            Open an agent
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="ph-scroll">
      {title}
      {header}
      {cards}
      <TeamGroup bridge={bridge} bridgeReady={bridgeReady} />
      <DigestCard bridge={bridge} bridgeReady={bridgeReady} onOpen={onOpen} />
      {groupPanes(panes).map((group) => (
        <section key={group.id} aria-label={group.title}>
          <h2 className="ph-sec">{group.title}</h2>
          <ul className="ph-group">
            {group.panes.map((pane) => {
              const lines = previews[pane.id] ?? [];
              const last = ago(pane.last_output_at, now);
              const signal = stuck?.get(pane.id);
              const sub = signal ? `Looks stuck: ${signal.summary}` : (lines[lines.length - 1] ?? 'No output yet');
              return (
                <li key={pane.id}>
                  <button type="button" onClick={() => onOpen(pane.id)} className={`ph-row${group.id === 'needs' ? ' ph-row--hot' : ''}`}>
                    <span className="ph-glyph">
                      <DotSpinner status={glyphStatus(pane.status)} />
                    </span>
                    <span className="ph-txt">
                      <span className="ph-t">{labels.get(pane.id) ?? pane.agent_type}</span>
                      <span className="ph-s">{projectName(pane.cwd)}</span>
                      <span className="ph-s ph-s--mono">{sub}</span>
                    </span>
                    {last && <span className="ph-meta">{last}</span>}
                    <Chevron />
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
