// "Scores on your code": which agent works best in this project, from your own history.
// One dense table, a scope and window switcher, and plain notes about what is counted.
// No folder path, pane id or chat text: the backend never sends them (`scores`). No ranking, no stars, no overall score.
import { useEffect, useState } from 'react';
import {
  AHEAD_TITLE,
  SCORE_WINDOWS,
  detailLine,
  notEnoughText,
  percentText,
  perMergedText,
  rateText,
  sampleHint,
  scoresApi,
  windowLabel,
  type ScoreAheadKey,
  type ScoreRate,
  type ScoreRow,
  type ScoresApi,
  type ScoresReport,
} from '../../lib/agent-scores';
import '../../styles/cockpit-scores.css';

type Scope = 'project' | 'all';

/** The table columns after the agent name. Captions are the one-line explanations under each header. */
const COLUMNS = [
  { id: 'turns', label: 'Turns', caption: 'Prompts you sent' },
  { id: 'endedOk', label: 'Ended OK', caption: 'No crash or API error. This is reliability, not code quality.' },
  { id: 'checksGreen', label: 'Checks green', caption: 'Only runs that had checks.' },
  { id: 'merged', label: 'Merged', caption: 'Separate copies you kept, out of kept + thrown away.' },
  { id: 'undone', label: 'Undone', caption: 'Turns you undid with Undo. Fewer is better.' },
  { id: 'perMerged', label: 'Per merged change', caption: 'Typical turns and working time. Needs 3 merged.' },
  { id: 'askedYou', label: 'Asked you', caption: 'Turns where you had to answer. Depends on your permission settings, so it is not a score.' },
] as const;

/** Read by the "How to read this" list: one short line per column that needs explaining. */
const HOW_TO_READ = [
  'Ended OK: the turn did not crash or hit an API error. It says nothing about the code.',
  'Checks green: runs that had checks, and all of them passed. Checks only count when a run starts with them.',
  'Merged: separate copies you kept, out of the copies you kept or threw away. Copies still waiting are not counted.',
  'Undone: turns that changed files and that you later undid with Undo. Fewer is better.',
  'Per merged change: typical turns and working time of merged runs. Shown from 3 merged runs.',
  'Asked you: turns where you had to answer a prompt. It depends on your permission settings, so it is not ranked.',
];

const HOW_TO_READ_END = [
  'Merged is your choice. It is not proof the code is right.',
  'Different tasks are not equal. A hard task makes an agent look worse.',
  'New versions of an agent change its numbers; this shows your past.',
  'Agents never see this page.',
];

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function cls(...names: (string | false | null | undefined)[]): string | undefined {
  const out = names.filter(Boolean).join(' ');
  return out === '' ? undefined : out;
}

/** One rate cell: "8 of 10", a muted "early" tag, the steady percent, or a dash with the reason. */
function RateCell({ rate, ahead, minSample }: { rate: ScoreRate; ahead: boolean; minSample: number }) {
  const hint = sampleHint(rate, minSample);
  if (hint !== null) {
    return (
      <td className="is-num is-none" title={hint}>
        -
      </td>
    );
  }
  const pct = percentText(rate);
  return (
    <td className={cls('is-num', rate.sample === 'early' && 'is-early')}>
      {rateText(rate)}
      {pct !== null && <span className="ck-scores-pct">{pct}</span>}
      {rate.sample === 'early' && <span className="ck-scores-tag">early</span>}
      {ahead && (
        <span className="ck-scores-ahead" title={AHEAD_TITLE}>
          ahead
        </span>
      )}
    </td>
  );
}

function ScoreTableRow({ row, minSample }: { row: ScoreRow; minSample: number }) {
  const ahead = new Set<ScoreAheadKey>(row.ahead);
  const detail = detailLine(row);
  const name = (
    <th scope="row">
      <span className="ck-scores-agent">{row.agentLabel}</span>
      <span className="ck-scores-account">{row.accountLabel}</span>
      {detail !== '' && <span className="ck-scores-detail">{detail}</span>}
    </th>
  );
  if (!row.enough) {
    return (
      <tr className="ck-scores-notenough">
        {name}
        <td colSpan={COLUMNS.length}>{notEnoughText(row)}</td>
      </tr>
    );
  }
  const perMerged = perMergedText(row.perMerged);
  return (
    <tr>
      {name}
      <td className="is-num">{row.turns}</td>
      <RateCell rate={row.endedOk} ahead={ahead.has('endedOk')} minSample={minSample} />
      <RateCell rate={row.checksGreen} ahead={ahead.has('checksGreen')} minSample={minSample} />
      <RateCell rate={row.merged} ahead={ahead.has('merged')} minSample={minSample} />
      <RateCell rate={row.undone} ahead={ahead.has('undone')} minSample={minSample} />
      {perMerged === null ? (
        <td className="is-num is-none" title="Needs 3 merged runs.">
          -
        </td>
      ) : (
        <td className="is-num">{perMerged}</td>
      )}
      <RateCell rate={row.askedYou} ahead={false} minSample={minSample} />
    </tr>
  );
}

export function AgentScores({
  api = scoresApi,
  project,
  onClose,
}: {
  api?: ScoresApi;
  /** The folder of the selected pane. Only the name is shown; the path goes to the backend. */
  project?: { path: string; name: string } | null;
  onClose: () => void;
}) {
  const [scope, setScope] = useState<Scope>(project ? 'project' : 'all');
  const [projectKey, setProjectKey] = useState<string | undefined>(project?.path);
  const [days, setDays] = useState<number | null>(90);
  const [report, setReport] = useState<ScoresReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .report(scope === 'project' ? projectKey : undefined, days)
      .then((next) => {
        if (!live) return;
        setReport(next);
        setError(null);
      })
      .catch((e) => live && setError(errText(e)));
    return () => {
      live = false;
    };
  }, [api, scope, projectKey, days]);

  const projectName = project?.name ?? report?.project?.label;
  const projects = report?.projects ?? [];
  const enoughCount = report ? report.rows.filter((r) => r.enough).length : 0;

  return (
    <div className="ck-scores">
      <header className="ck-memory-head">
        <div>
          <h1 className="ck-title">Scores on your code</h1>
          <p className="ck-sub">Which agent works best here, from your own history.</p>
        </div>
        <button type="button" className="ck-btn ck-btn--plain" onClick={onClose}>
          Close
        </button>
      </header>

      <div className="ck-scores-controls">
        <div className="ck-scores-scope" role="group" aria-label="Scope">
          <button
            type="button"
            aria-pressed={scope === 'project'}
            className={cls(scope === 'project' && 'is-on')}
            disabled={!project}
            onClick={() => setScope('project')}
          >
            {projectName ? `This project: ${projectName}` : 'This project'}
          </button>
          <button
            type="button"
            aria-pressed={scope === 'all'}
            className={cls(scope === 'all' && 'is-on')}
            onClick={() => setScope('all')}
          >
            All projects
          </button>
        </div>

        <div className="ck-scores-scope" role="group" aria-label="Filters">
          {scope === 'project' && projects.length > 0 && (
            <select
              className="ck-scores-select"
              aria-label="Project"
              value={report?.project?.id ?? ''}
              onChange={(e) => setProjectKey(e.target.value)}
            >
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          )}
          <select
            className="ck-scores-select"
            aria-label="Window"
            value={days === null ? 'all' : String(days)}
            onChange={(e) => setDays(e.target.value === 'all' ? null : Number(e.target.value))}
          >
            {SCORE_WINDOWS.map((d) => (
              <option key={d ?? 'all'} value={d === null ? 'all' : String(d)}>
                {windowLabel(d)}
              </option>
            ))}
          </select>
        </div>
      </div>

      {error && (
        <p className="ck-scores-error" role="alert">
          {error}
        </p>
      )}

      {report === null && error === null && <p className="ck-scores-empty">Loading</p>}

      {report !== null && report.rows.length === 0 && (
        <p className="ck-scores-empty">No turns here yet. Run an agent in this project, then come back.</p>
      )}

      {report !== null && report.rows.length > 0 && (
        <>
          <div className="ck-scores-scroll">
            <table className="ck-scores-table">
              <thead>
                <tr>
                  <th scope="col">Agent</th>
                  {COLUMNS.map((c) => (
                    <th key={c.id} scope="col" className="is-num" aria-describedby={`ck-scores-cap-${c.id}`}>
                      {c.label}
                      <span className="ck-scores-caption" id={`ck-scores-cap-${c.id}`}>
                        {c.caption}
                      </span>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {report.rows.map((row) => (
                  <ScoreTableRow key={`${row.agentId}\u0000${row.accountId ?? ''}`} row={row} minSample={report.minSample} />
                ))}
              </tbody>
            </table>
          </div>
          {enoughCount === 1 && <p className="ck-scores-empty">Only one agent has enough runs here, so nothing can be compared yet.</p>}
        </>
      )}

      <details className="ck-scores-how">
        <summary>How to read this</summary>
        <ul>
          {HOW_TO_READ.map((line) => (
            <li key={line}>{line}</li>
          ))}
          {HOW_TO_READ_END.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      </details>

      {report !== null && report.notes.length > 0 && (
        <ul className="ck-scores-notes">
          {report.notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
