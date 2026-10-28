// "Your plans" on the phone (read only): one card per plan (agent · account) with its turns, time, files and limits,
// a month switcher and the plain notes. The numbers come from the PC's `/plans` route (`bridgePlansApi`), the same
// report the desktop page reads. No folder and no chat text: the report has none.
import { useEffect, useState } from 'react';
import {
  filesText,
  formatBusy,
  limitsText,
  neighbour,
  type PlanRow,
  type PlansApi,
  type PlansReport,
} from '../../lib/plans-report';
import { Sheet } from './MobileSheet';
import '../../styles/ph-plans.css';

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function turnsWord(n: number): string {
  return n === 1 ? '1 turn' : `${n} turns`;
}

function PlanCard({ row }: { row: PlanRow }) {
  return (
    <li className="ph-plans-card">
      <p className="ph-plans-name">
        {row.agentLabel} · {row.accountLabel}
      </p>
      <p className="ph-plans-stats">
        {turnsWord(row.turns)} · {formatBusy(row.busyMs)}
      </p>
      <p className="ph-plans-line">{filesText(row.files)}</p>
      <p className="ph-plans-line ph-plans-line--quiet">{limitsText(row.limits)}</p>
    </li>
  );
}

export function PhonePlans({ api, onClose }: { api: PlansApi; onClose: () => void }) {
  const [month, setMonth] = useState<string | undefined>(undefined);
  const [report, setReport] = useState<PlansReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .report(month)
      .then((next) => {
        if (!live) return;
        setReport(next);
        setError(null);
      })
      .catch((e: unknown) => live && setError(errText(e)));
    return () => {
      live = false;
    };
  }, [api, month]);

  const months = report?.months ?? [];
  const here = report?.month.id ?? '';
  const older = report ? neighbour(months, here, 'older') : null;
  const newer = report ? neighbour(months, here, 'newer') : null;

  return (
    <Sheet title="Your plans" onClose={onClose}>
      <div className="ph-plans">
        <div className="ph-plans-months" role="group" aria-label="Month">
          <button
            type="button"
            className="ph-plans-icon"
            aria-label="Earlier month"
            disabled={older === null}
            onClick={() => older && setMonth(older)}
          >
            {'‹'}
          </button>
          <p className="ph-plans-month" aria-live="polite">
            {report?.month.label ?? 'Loading'}
          </p>
          <button
            type="button"
            className="ph-plans-icon"
            aria-label="Later month"
            disabled={newer === null}
            onClick={() => newer && setMonth(newer)}
          >
            {'›'}
          </button>
        </div>

        {error && (
          <p className="ph-foot ph-foot--err" role="alert">
            {error}
          </p>
        )}

        {report !== null && report.rows.length === 0 && <p className="ph-plans-empty">No turns this month yet.</p>}

        {report !== null && report.rows.length > 0 && (
          <>
            <ul className="ph-plans-list" aria-label="Plans">
              {report.rows.map((row) => (
                <PlanCard key={`${row.agentId}\u0000${row.accountId ?? ''}`} row={row} />
              ))}
            </ul>
            <p className="ph-plans-total">
              All plans · {turnsWord(report.totals.turns)} · {formatBusy(report.totals.busyMs)}
            </p>
          </>
        )}

        {report !== null && report.notes.length > 0 && (
          <ul className="ph-plans-notes">
            {report.notes.map((note) => (
              <li key={note}>{note}</li>
            ))}
          </ul>
        )}
      </div>
    </Sheet>
  );
}
