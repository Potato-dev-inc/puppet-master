import type { TurnSummary } from '../../lib/cockpit-api';
import { formatElapsed } from './model';
import { inlineCode } from './inline-code';

/**
 * The "Last turn" block of the Receipt tab: one plain sentence and a meta line. The sentence carries
 * this turn's own file counts, so the meta line leaves them out: two counts side by side read as a
 * contradiction. Renders nothing before the first turn (the Receipt shows its own empty state).
 */
export function LastTurn({ summary, now }: { summary: TurnSummary | undefined; now: number }) {
  if (!summary) return null;
  const files = summary.files;
  const facts = [`Turn ${summary.turn}`, formatElapsed(summary.durationMs), `${formatElapsed(now - summary.endedAtMs)} ago`];
  return (
    <section className="ck-lastturn" aria-label="Last turn">
      <div className="ck-caption">Last turn</div>
      <p className="ck-lastturn-line" title={files?.paths.join('\n') || undefined}>
        {inlineCode(summary.sentence)}
      </p>
      <p className="ck-sub">{facts.join(' · ')}</p>
    </section>
  );
}
