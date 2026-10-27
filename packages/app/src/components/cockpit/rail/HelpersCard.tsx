// The panes this pane started (its helpers): a status dot, the title and the agent. A click opens that pane.
import { DotSpinner } from '../DotSpinner';
import { spinnerStatus } from '../model';
import { RailCard } from './RailCard';

export interface HelperRow {
  id: string;
  title: string;
  agent: string | null;
  status: string;
}

export function HelpersCard({ helpers, onOpen }: { helpers: readonly HelperRow[]; onOpen: (id: string) => void }) {
  if (helpers.length === 0) return null;
  return (
    <RailCard title="Helpers" note={String(helpers.length)} testId="rail-helpers">
      <ul className="ck-rail-list">
        {helpers.map((h) => (
          <li key={h.id}>
            <button type="button" className="ck-rail-item" onClick={() => onOpen(h.id)} title={`Open ${h.title}`}>
              <DotSpinner status={spinnerStatus(h.status)} />
              <span className="ck-rail-file">{h.title}</span>
              {h.agent && <span className="ck-rail-folder">{h.agent}</span>}
            </button>
          </li>
        ))}
      </ul>
    </RailCard>
  );
}
