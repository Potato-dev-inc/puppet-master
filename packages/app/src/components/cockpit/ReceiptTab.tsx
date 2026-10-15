import type { ReactNode } from 'react';
import type { PaneRunInfo, TurnSummary } from '../../lib/cockpit-api';
import { LastTurn } from './LastTurn';
import { receiptSummary, type CopyTotals } from './receipt-view';

/**
 * The Receipt tab: what was checked, what the last turn did, a few facts, and the rest under
 * Details. File counts come from one place (see `receiptSummary`). Nothing here is a bordered table.
 */
export function ReceiptTab({
  info,
  error,
  lastTurn,
  now,
  copy,
  agentName,
  secondOpinion,
}: {
  /** `undefined` while loading, `null` for a pane with no run. */
  info: PaneRunInfo | null | undefined;
  error: string | null;
  lastTurn: TurnSummary | undefined;
  now: number;
  /** What the pane's separate copy holds right now, when it works in one. */
  copy: CopyTotals | null;
  agentName: string;
  /** The Second opinion block, shown once there is a turn to review. */
  secondOpinion: ReactNode;
}) {
  if (error) {
    return (
      <div className="ck-empty">
        <h2 className="ck-section">Receipt unavailable</h2>
        <p className="ck-sub">{error}</p>
      </div>
    );
  }
  if (info === undefined) return <p className="ck-sub ck-pad">Loading…</p>;

  const receipt = info?.receipt ?? null;
  const hasReceipt = receipt !== null;
  if (!lastTurn && !hasReceipt) {
    return (
      <div className="ck-empty">
        <h2 className="ck-section">No turns yet</h2>
        <p className="ck-sub">The receipt appears when {agentName} finishes its first turn.</p>
      </div>
    );
  }

  const summary = hasReceipt ? receiptSummary(receipt, copy, now) : null;
  return (
    <>
      <div className="ck-receipt">
        {summary && (
          <section className="ck-rc-block" aria-label="Checks">
            <div className="ck-caption">Checks</div>
            <p className="ck-rc-line">{summary.checks}</p>
          </section>
        )}
        {lastTurn && <LastTurn summary={lastTurn} now={now} />}
        {summary && summary.facts.length > 0 && (
          <dl className="ck-facts" aria-label="Facts">
            {summary.facts.map((row) => (
              <div className="ck-facts-row" key={row.key}>
                <dt>{row.key}</dt>
                <dd>{row.value}</dd>
              </div>
            ))}
          </dl>
        )}
        {info && (
          <details className="ck-details">
            <summary>Details</summary>
            <dl className="ck-facts ck-facts--details">
              {summary?.details.map((row, i) => (
                <div className="ck-facts-row" key={`${row.key}-${i}`}>
                  <dt>{row.key}</dt>
                  <dd>{row.value}</dd>
                </div>
              ))}
              <div className="ck-facts-row">
                <dt>Run ID</dt>
                <dd className="ck-mono">{info.handle}</dd>
              </div>
            </dl>
          </details>
        )}
      </div>
      {secondOpinion}
    </>
  );
}
