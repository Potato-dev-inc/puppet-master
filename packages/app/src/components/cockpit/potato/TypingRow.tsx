// "Fish is typing…" with the cockpit's dot spinner (it holds still under reduced motion).
import { COPY } from '../../../lib/potato';
import { DotSpinner } from '../DotSpinner';

export function TypingRow({ name }: { name: string }) {
  return (
    <div className="ck-potato-typing" role="status">
      <DotSpinner status="running" label="Typing" />
      <span>{COPY.typing(name)}</span>
    </div>
  );
}
