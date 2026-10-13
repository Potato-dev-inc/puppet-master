// Phone: a quiet note under the tab bar when the loop guard says the open agent looks stuck
// (same error again and again, no progress, no output for a while). The PC shows the full card.
import type { LoopSignalInfo } from '../../lib/bridge-types';

export function StuckNote({ signal }: { signal: LoopSignalInfo }) {
  return (
    <div className="ph-note ph-note--stuck" role="note">
      <b>Looks stuck</b>
      <span>{signal.summary}</span>
    </div>
  );
}
