// The Agent Store page inside the cockpit: the grid, and on top of it the detail sheet or the Check sheet of
// one agent. It owns only "which sheet is open"; every install, add and check goes through the Store api.
import { useState } from 'react';
import { agentStoreApi, type AgentStoreApi } from '../../lib/agent-store';
import { AgentDetail } from './AgentDetail';
import { AgentStore } from './AgentStore';
import { StoreCheck, type RunStoreCheck } from './StoreCheck';

export interface StoreHostProps {
  api?: AgentStoreApi;
  /** Opens the agent in its own pane (the person signs in there). */
  onOpenAgent: (id: string) => void;
  onBack: () => void;
  /** Runs one Check. Without it the sheet has no "Check it works" button. */
  runCheck?: RunStoreCheck;
}

type Sheet = { kind: 'detail'; id: string } | { kind: 'check'; id: string; label: string; version: string | null };

export function StoreHost({ api = agentStoreApi, onOpenAgent, onBack, runCheck }: StoreHostProps) {
  const [sheet, setSheet] = useState<Sheet | null>(null);
  // Bumped when something changed on the list, so the grid reloads its cards.
  const [revision, setRevision] = useState(0);

  const startCheck = (id: string) => {
    // The label and version come from the Store itself; the sheet shows the plain id until they arrive.
    api
      .detail(id)
      .then((d) => setSheet({ kind: 'check', id, label: d.label, version: d.installed_version }))
      .catch(() => setSheet({ kind: 'check', id, label: id, version: null }));
  };

  return (
    <>
      <AgentStore revision={revision} api={api} onOpenAgent={(id) => setSheet({ kind: 'detail', id })} onBack={onBack} />
      {sheet?.kind === 'detail' && (
        <AgentDetail
          id={sheet.id}
          api={api}
          onOpenAgent={(id) => {
            setSheet(null);
            onOpenAgent(id);
          }}
          onChanged={() => setRevision((n) => n + 1)}
          onClose={() => setSheet(null)}
          onCheck={runCheck ? startCheck : undefined}
        />
      )}
      {sheet?.kind === 'check' && runCheck && (
        <StoreCheck
          id={sheet.id}
          label={sheet.label}
          installedVersion={sheet.version}
          runCheck={runCheck}
          onOpenAgent={(id) => {
            setSheet(null);
            onOpenAgent(id);
          }}
          onClose={() => {
            setSheet(null);
            setRevision((n) => n + 1);
          }}
        />
      )}
    </>
  );
}
