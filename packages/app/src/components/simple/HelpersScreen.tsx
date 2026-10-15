// Set up agents inside Simple mode. Sign in and Install open an ordinary window (an agent pane that is
// not a task), so this lives apart from the files that may never start a pane (guards.test.ts).
import type { CockpitApi } from '../../hooks/useCockpit';
import { AgentDoctor } from '../cockpit/AgentDoctor';

export function HelpersScreen({ c, onClose }: { c: CockpitApi; onClose: () => void }) {
  return (
    <div className="sx-helpers">
      <AgentDoctor
        onOpenAgent={(id) => void c.newAgent(id)}
        onNotify={c.notify}
        onShowPane={(id) => c.select(id)}
        onClose={onClose}
      />
    </div>
  );
}
