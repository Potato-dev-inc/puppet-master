// The panes this pane started (its helpers): a status dot, the title and the agent. A click opens that pane.
// With `usage`, a roll-up line for the whole tree and a usage line under each helper (turns, busy time, tokens
// only when the chat reported them). `onSaveTeam` adds the "Save this team" button (crews).
import type { PaneUsage } from '../../../lib/pane-usage';
import { DotSpinner } from '../DotSpinner';
import { spinnerStatus } from '../model';
import { RailCard } from './RailCard';
import { helperUsageLine, rollupLine } from './usageText';

export interface HelperRow {
  id: string;
  title: string;
  agent: string | null;
  status: string;
}

export function HelpersCard({
  helpers,
  usage,
  onOpen,
  onSaveTeam,
}: {
  helpers: readonly HelperRow[];
  usage?: PaneUsage | null;
  onOpen: (id: string) => void;
  /** Shown as "Save this team" under the list: the pane and its helpers become a crew. */
  onSaveTeam?: () => void;
}) {
  if (helpers.length === 0) return null;
  const byId = new Map((usage?.helpers ?? []).map((u) => [u.paneId, u] as const));
  return (
    <RailCard title="Helpers" note={String(helpers.length)} testId="rail-helpers">
      {usage && <p className="ck-rail-line" data-testid="rail-helpers-rollup">{rollupLine(usage)}</p>}
      <ul className="ck-rail-list">
        {helpers.map((h) => {
          const line = byId.get(h.id);
          const text = line ? helperUsageLine(h.agent, line) : null;
          return (
            <li key={h.id}>
              <button type="button" className="ck-rail-item" onClick={() => onOpen(h.id)} title={`Open ${h.title}`}>
                <DotSpinner status={spinnerStatus(h.status)} />
                <span className="ck-rail-file">{h.title}</span>
                {h.agent && <span className="ck-rail-folder">{h.agent}</span>}
              </button>
              {text && <p className="ck-rail-line" title={text}>{text}</p>}
            </li>
          );
        })}
      </ul>
      {onSaveTeam && (
        <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm ck-rail-action" onClick={onSaveTeam} title="Save this agent and its helpers as a crew you can start again in any folder">
          Save this team
        </button>
      )}
    </RailCard>
  );
}
