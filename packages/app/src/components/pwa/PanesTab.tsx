// "Panes" tab of the mobile PWA: pane list, or the selected pane's terminal.
import type { PaneInfo } from '@puppet-master/shared';
import type { useBridgePaneRegistry } from '../../hooks/useBridgePaneRegistry';
import { BridgePaneTerminal, useBridgePaneTransport } from '../BridgePaneTerminal';

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-pm-ok',
  waiting_input: 'bg-pm-accent',
  idle: 'bg-pm-muted',
  error: 'bg-pm-err',
};

export function PanesTab({
  panes,
  bridgeReady,
  registry,
  mobileInputDelayMs,
  mobileInputVisible,
  selected,
  onSelect,
}: {
  panes: PaneInfo[];
  bridgeReady: boolean;
  registry: ReturnType<typeof useBridgePaneRegistry>;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
  selected: string | null;
  onSelect: (paneId: string | null) => void;
}) {
  const selectedPane = selected ? registry.panes.get(selected) : undefined;
  const selectedTransport = useBridgePaneTransport(registry.makeTransport, selected ?? 'pane-placeholder');

  return (
    <div className="flex flex-col h-full min-h-0">
      {selected && selectedPane ? (
        <BridgePaneTerminal
          pane={selectedPane.info}
          status={selectedPane.status}
          subscribePaneData={registry.subscribePaneData}
          transport={selectedTransport}
          title={selectedPane.info.agent_type}
          mobileInputDelayMs={mobileInputDelayMs}
          mobileInputVisible={mobileInputVisible}
        />
      ) : (
      <div className="flex-1 min-h-0 overflow-y-auto pt-16">
        {!bridgeReady && (
          <div className="text-xs text-pm-muted text-center py-6">Connecting to desktop…</div>
        )}
        {bridgeReady && panes.length === 0 && (
          <div className="text-xs text-pm-muted text-center py-6 px-4">
            No panes yet. Open the menu to spawn an agent on your PC.
          </div>
        )}
        {panes.map((pane) => (
          <div
            key={pane.id}
            className={`flex items-center gap-1 border-b border-pm-border/50 ${selected === pane.id ? 'bg-pm-raised' : ''}`}
          >
            <button
              type="button"
              onClick={() => onSelect(pane.id)}
              className="flex-1 min-w-0 flex items-center gap-2 px-3 py-2.5 text-left hover:bg-pm-raised/60 text-xs"
            >
              <span className={`w-2 h-2 rounded-full flex-shrink-0 ${STATUS_COLOR[pane.status] ?? 'bg-pm-muted'}`} />
              <span className="font-mono text-zinc-200 truncate flex-1">{pane.agent_type}</span>
              <span className="text-pm-muted text-[10px]">{pane.status}</span>
            </button>
          </div>
        ))}
      </div>
      )}
    </div>
  );
}
