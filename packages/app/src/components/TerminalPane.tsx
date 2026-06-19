import type { HTMLAttributes } from 'react';
import { useState } from 'react';
import { listPresets, type AgentType } from '@puppet-master/shared';
import { useTerminalSession } from '../hooks/useTerminalSession';
import type { PaneData } from '../hooks/usePaneRegistry';

interface Props {
  pane: PaneData;
  subscribePaneData: (paneId: string, cb: (data: Uint8Array) => void) => () => void;
  onClose: (paneId: string) => void;
  onSwitchAgent: (paneId: string, agentType: AgentType) => Promise<void>;
  dragHandleProps?: HTMLAttributes<HTMLButtonElement>;
}

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-pm-ok',
  waiting_input: 'bg-pm-warn',
  idle: 'bg-pm-muted',
  error: 'bg-pm-err',
};

export function TerminalPane({
  pane,
  subscribePaneData,
  onClose,
  onSwitchAgent,
  dragHandleProps,
}: Props) {
  const [switching, setSwitching] = useState(false);
  const containerRef = useTerminalSession({
    paneId: pane.info.id,
    sessionKey: pane.info.created_at,
    subscribePaneData,
    });

  const agent = pane.info.agent_type as AgentType;
  const presets = listPresets();
  const preset = presets.find((p) => p.type === agent);

  const handleAgentChange = async (next: AgentType) => {
    if (next === agent || switching) return;
    setSwitching(true);
    try {
      await onSwitchAgent(pane.info.id, next);
    } finally {
      setSwitching(false);
    }
  };

  return (
    <div className="flex flex-col h-full rounded border border-pm-border bg-pm-bg overflow-hidden relative">
      {switching && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-pm-bg/80 text-xs text-pm-muted">
          Switching agent…
        </div>
      )}
      <div className="flex items-center gap-2 px-2 py-1 border-b border-pm-border bg-pm-panel text-xs text-pm-text">
        <button
          type="button"
          className="cursor-grab active:cursor-grabbing px-1 text-pm-muted hover:text-pm-text"
          title="Drag to reorder pane"
          aria-label="Drag to reorder pane"
          {...dragHandleProps}
        >
          ::
        </button>
        <span
          className={`inline-block w-2 h-2 rounded-full ${STATUS_COLOR[pane.status] ?? 'bg-pm-muted'}`}
          title={pane.status}
        />
        <select
          className="bg-pm-bg border border-pm-border rounded px-1 py-0.5 text-xs disabled:opacity-50"
          value={agent}
          disabled={switching}
          onChange={(e) => void handleAgentChange(e.target.value as AgentType)}
        >
          {presets.map((p) => (
            <option key={p.type} value={p.type}>
              {p.label}
            </option>
          ))}
        </select>
        <span className="text-pm-muted truncate" title={pane.info.cwd}>
          {preset?.label ?? pane.info.agent_type} · pid {pane.info.pid}
        </span>
        <div className="flex-1" />
        <button
          className="px-1.5 py-0.5 rounded text-pm-muted hover:text-pm-err hover:bg-pm-err/10"
          title="Kill pane"
          onClick={() => onClose(pane.info.id)}
        >
          ✕
        </button>
      </div>
      <div ref={containerRef} className="flex-1 min-h-0 overflow-hidden terminal-host" />
    </div>
  );
}
