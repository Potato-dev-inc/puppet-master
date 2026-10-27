// How full the agent's context window is: a thin bar and "48k of 200k tokens (24%)". When the agent does not
// report its window we say so; we never guess.
import type { UsageItem } from '../../../lib/sessionChatModel';
import { RailCard } from './RailCard';
import { formatTokens, readContext } from './railFormat';

export function ContextCard({ usage }: { usage: UsageItem | null }) {
  const ctx = readContext(usage);
  if (!ctx) {
    return (
      <RailCard title="Context" testId="rail-context">
        <p className="ck-rail-empty">Not reported by this agent</p>
      </RailCard>
    );
  }
  const tip = `Input ${ctx.input.toLocaleString('en-US')} tokens, output ${ctx.output.toLocaleString('en-US')} tokens`;
  return (
    <RailCard title="Context" note={`${ctx.percent}%`} testId="rail-context">
      <div className="ck-rail-bar" role="meter" aria-label="Context used" aria-valuemin={0} aria-valuemax={100} aria-valuenow={ctx.percent} title={tip}>
        <span className="ck-rail-bar-fill" style={{ width: `${ctx.percent}%` }} />
      </div>
      <p className="ck-rail-line" title={tip}>
        {formatTokens(ctx.used)} of {formatTokens(ctx.window)} tokens ({ctx.percent}%)
      </p>
    </RailCard>
  );
}
