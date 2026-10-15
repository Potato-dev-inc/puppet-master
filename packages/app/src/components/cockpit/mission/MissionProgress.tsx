// A mission's Progress (DESIGN-MISSIONS 6.3, 6.4, 6.6): the state line and budget, the one 2px
// progress bar, the timeline (one hairline row per turn or event, newest first), the card when it
// needs you, and the Done review when it is finished. Notes renders MISSION.md's agents' notes.
import { useEffect, useState } from 'react';
import { barText, isActive, tauriMissionsApi, type MissionView, type MissionsApi, type TimelineRow } from '../../../lib/missions';
import { SegTabs } from '../SegTabs';
import { Sheet } from '../Sheet';
import { MissionCard } from './MissionCards';
import { MissionGlyph, PreviewTag } from './MissionGlyph';
import '../../../styles/missions.css';

const SHOWN = 8;

function Row({ row, onOpenPane }: { row: TimelineRow; onOpenPane?: (paneId: string) => void }) {
  const [open, setOpen] = useState(false);
  const checks = row.checks ?? [];
  const action =
    row.action === 'open_pane' && row.pane_id && onOpenPane ? (
      <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => onOpenPane(row.pane_id!)}>
        Open pane
      </button>
    ) : null;
  const body = (
    <>
        <MissionGlyph glyph={row.glyph} />
        {row.kind === 'turn' ? (
          <>
            <span className="ck-mission-turn">Turn {row.turn}</span>
            <span className="ck-mission-agent-name">{row.agent}</span>
            <span className="ck-mission-counts">
              {row.counts ?? ''}
              {row.delta !== undefined && <span className="ck-mission-delta"> ({row.delta})</span>}
            </span>
            <span className="ck-mission-phrase">{row.text}</span>
          </>
        ) : (
          <span className="ck-mission-event">{row.text}</span>
        )}
    </>
  );
  return (
    <li className={`ck-mission-row ck-mission-row--${row.kind}`}>
      {checks.length > 0 ? (
        <button type="button" className="ck-mission-row-main" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
          {body}
        </button>
      ) : (
        <div className="ck-mission-row-main ck-mission-row-main--static">{body}</div>
      )}
      {action}
      {open && checks.length > 0 && (
        <ul className="ck-mission-row-checks">
          {checks.map((c) => (
            <li key={c.name}>
              <MissionGlyph glyph={c.passed ? 'v' : 'x'} />
              <span className="ck-mission-check-name">{c.name}</span>
              <span className="ck-mission-hint">{c.text}</span>
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

/** The Done review (DESIGN-MISSIONS 6.6). Nothing has been merged: the person reviews the copy. */
export function MissionDone({ m, onReview }: { m: MissionView; onReview?: () => void }) {
  return (
    <section className="ck-mission-done" aria-label="Mission done">
      <div className="ck-mission-done-head">
        <MissionGlyph glyph={m.glyph} />
        <span className="ck-row-title">{m.state_text}</span>
      </div>
      <dl className="ck-kv ck-mission-kv">
        <div className="ck-kv-row">
          <dt>Checks</dt>
          <dd>
            {m.checks.map((c) => `${c.name} ${c.passed ? 'pass' : 'fail'}`).join(' · ')}
            {m.start ? <span className="ck-mission-hint"> ({m.start} at the start)</span> : null}
          </dd>
        </div>
        <div className="ck-kv-row">
          <dt>Work</dt>
          <dd>{m.work_text}</dd>
        </div>
        <div className="ck-kv-row">
          <dt>Test changes</dt>
          <dd>{m.test_changes_text}</dd>
        </div>
        {m.budget.text && (
          <div className="ck-kv-row">
            <dt>Budget</dt>
            <dd>{m.budget.text}</dd>
          </div>
        )}
        {m.copy && (
          <div className="ck-kv-row">
            <dt>Where</dt>
            <dd>
              {m.project_name} · separate copy from {m.copy.base}
            </dd>
          </div>
        )}
      </dl>
      {onReview && (
        <div className="ck-mission-done-actions">
          <button type="button" className="ck-btn ck-btn--primary" onClick={onReview}>
            Review changes
          </button>
        </div>
      )}
      <p className="ck-mission-foot">Nothing has been merged. The separate copy stays until you merge or discard it.</p>
    </section>
  );
}

export function MissionProgress({
  mission,
  api = tauriMissionsApi,
  onChanged,
  onOpenPane,
  onNotify,
  withCard = true,
}: {
  mission: MissionView;
  api?: MissionsApi;
  onChanged?: (m: MissionView) => void;
  onOpenPane?: (paneId: string) => void;
  onNotify?: (text: string) => void;
  /** Show the card when it needs you (the Inbox lists the cards above, so it passes false). */
  withCard?: boolean;
}) {
  const m = mission;
  const [tab, setTab] = useState<'progress' | 'notes'>('progress');
  const [all, setAll] = useState(false);
  const [confirmStop, setConfirmStop] = useState(false);
  const [notes, setNotes] = useState<string | null>(null);
  const turns = m.timeline.filter((r) => r.kind === 'turn').length;
  const rows = all ? m.timeline : m.timeline.slice(0, SHOWN);
  const pct = m.total > 0 ? Math.round((m.passed / m.total) * 100) : 0;
  const done = m.state === 'green' || m.state === 'handed_off';

  useEffect(() => {
    if (tab !== 'notes') return;
    let cancelled = false;
    api
      .get(m.id)
      .then((full) => !cancelled && setNotes(full.notes ?? ''))
      .catch(() => !cancelled && setNotes(''));
    return () => {
      cancelled = true;
    };
  }, [api, m.id, m.rev, tab]);

  const run = (what: Promise<MissionView>, sentence?: string) =>
    what
      .then((next) => {
        onChanged?.(next);
        if (sentence) onNotify?.(sentence);
      })
      .catch((err) => onNotify?.(err instanceof Error ? err.message : String(err)));

  return (
    <div className="ck-mission" data-testid="mission-progress">
      <header className="ck-mission-head">
        <div className="ck-mission-title-line">
          <h2 className="ck-mission-title" title={m.objective}>
            {m.title}
          </h2>
          <PreviewTag />
          <span className="ck-mission-state">
            <MissionGlyph glyph={m.glyph} />
            <span>{m.state_text}</span>
          </span>
          {m.budget.text && <span className="ck-mission-budget-chip">{m.budget.text}</span>}
        </div>
        <p className="ck-mission-sub">
          {m.project_name}
          {m.copy ? ` · separate copy from ${m.copy.base}` : ''}
          {m.agents_line ? ` · ${m.agents_line}` : ''}
          {m.started_by !== 'you' ? ` · Started by ${m.started_by}` : ''}
        </p>
        {isActive(m) && (
          <div className="ck-mission-actions">
            {m.state === 'paused' ? (
              <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={() => void run(api.resume(m.id), 'Resumed.')}>
                Resume
              </button>
            ) : m.pause_requested ? (
              <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={() => void run(api.resume(m.id))}>
                Keep going after this turn
              </button>
            ) : (
              <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={() => void run(api.pause(m.id), 'Pauses after this turn.')}>
                Pause after this turn
              </button>
            )}
            <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={() => setConfirmStop(true)}>
              Stop mission…
            </button>
          </div>
        )}
      </header>

      <div className="ck-tabs-bar">
        <SegTabs
          label="Mission view"
          value={tab}
          onChange={setTab}
          options={[
            { id: 'progress', label: 'Progress' },
            { id: 'notes', label: 'Notes' },
          ]}
        />
      </div>

      {tab === 'progress' ? (
        <div className="ck-mission-body">
          {withCard && m.state === 'needs_you' && m.needs && m.needs !== 'budget' && (
            <ul className="ck-list ck-q-list">
              <MissionCard m={m} api={api} onChanged={onChanged} onNotify={onNotify} />
            </ul>
          )}
          {done && <MissionDone m={m} onReview={m.current_pane && onOpenPane ? () => onOpenPane(m.current_pane!) : undefined} />}
          {m.total > 0 && (
            <div className="ck-mission-bar-line">
              <div className="ck-mission-bar" role="progressbar" aria-valuemin={0} aria-valuemax={m.total} aria-valuenow={m.passed} aria-label={barText(m)}>
                <span className="ck-mission-bar-fill" style={{ width: `${pct}%` }} />
              </div>
              <span className="ck-mission-bar-text">{barText(m)}</span>
              {m.budget.text && <span className="ck-mission-hint">{m.budget.text}</span>}
            </div>
          )}
          <ul className="ck-mission-rows" aria-label="Progress">
            {rows.map((r, i) => (
              <Row key={`${r.kind}-${r.turn ?? ''}-${r.at_ms}-${i}`} row={r} onOpenPane={onOpenPane} />
            ))}
          </ul>
          {!all && m.timeline.length > SHOWN && (
            <button type="button" className="ck-btn ck-btn--plain ck-btn--sm ck-mission-all" onClick={() => setAll(true)}>
              Show all {turns} turns
            </button>
          )}
        </div>
      ) : (
        <div className="ck-mission-body">
          <p className="ck-mission-hint">From MISSION.md in the separate copy. Agents add a few lines after each turn.</p>
          {notes === null ? (
            <p className="ck-sub">Loading…</p>
          ) : notes.trim() ? (
            <pre className="ck-mission-notes">{notes}</pre>
          ) : (
            <p className="ck-sub">No notes from agents yet.</p>
          )}
        </div>
      )}

      {confirmStop && (
        <Sheet labelledBy="ck-mission-stop-t" onClose={() => setConfirmStop(false)}>
          <h2 className="ck-section" id="ck-mission-stop-t">
            Stop this mission?
          </h2>
          <p className="ck-sub">
            {m.current_agent ? `${m.current_agent} stops now. ` : ''}Its separate copy and notes stay, so you can review or merge what&apos;s there.
          </p>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={() => setConfirmStop(false)}>
              Cancel
            </button>
            <button
              type="button"
              className="ck-btn ck-btn--primary"
              onClick={() => {
                setConfirmStop(false);
                void run(api.stop(m.id), 'Mission stopped. Its separate copy stays.');
              }}
            >
              Stop mission
            </button>
          </div>
        </Sheet>
      )}
    </div>
  );
}
