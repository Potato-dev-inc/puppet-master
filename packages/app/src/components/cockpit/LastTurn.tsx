import type { TurnSummary } from '../../lib/cockpit-api';
import { formatElapsed } from './model';

/** The "Last turn" block of the Receipt tab: one plain sentence plus the facts behind it. */
export function LastTurn({ summary, now }: { summary: TurnSummary | undefined; now: number }) {
  if (!summary) {
    return (
      <section className="ck-lastturn" aria-label="Last turn">
        <div className="ck-caption">Last turn</div>
        <p className="ck-sub">Nothing yet. A summary shows up when the agent finishes a turn.</p>
      </section>
    );
  }
  const files = summary.files;
  const facts = [
    `Turn ${summary.turn}`,
    formatElapsed(summary.durationMs),
    files ? `${files.count} ${files.count === 1 ? 'file' : 'files'}` : null,
    `${formatElapsed(now - summary.endedAtMs)} ago`,
  ].filter(Boolean);
  return (
    <section className="ck-lastturn" aria-label="Last turn">
      <div className="ck-caption">Last turn</div>
      <p className="ck-lastturn-line" title={files?.paths.join('\n') || undefined}>
        {summary.sentence}
      </p>
      <p className="ck-sub">{facts.join(' · ')}</p>
    </section>
  );
}
