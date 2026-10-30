// The helpers a pane's agent started through Puppet Master, as compact rows under the turn they were started in:
// "Helper: <task>", the agent's name, and a live status at the right. A row opens that helper's own pane.
import { HELPER_STATUS_LABEL, type ChatHelper } from '../../lib/chat-helpers';
import { DotSpinner } from '../cockpit/DotSpinner';

const DOT: Record<ChatHelper['status'], 'running' | 'waiting_input' | 'idle' | 'error'> = {
  working: 'running',
  'needs-you': 'waiting_input',
  done: 'idle',
  failed: 'error',
};

export function PaneHelperRows({ helpers, onOpen }: { helpers: readonly ChatHelper[]; onOpen?: (paneId: string) => void }) {
  if (helpers.length === 0) return null;
  return (
    <div className="space-y-0.5" data-testid="pane-helper-rows">
      {helpers.map((h) => (
        <div key={h.id} className="ml-1 border-l border-pm-border pl-2 ck-chat-helper" data-testid="pane-helper-row" data-status={h.status}>
          <button
            type="button"
            onClick={() => onOpen?.(h.id)}
            disabled={!onOpen}
            title={onOpen ? `Open ${h.title}` : h.title}
            className="flex w-full min-w-0 items-center gap-2 rounded px-1 py-0.5 text-left hover:bg-pm-raised ck-chat-tool ck-chat-helper-row"
          >
            <DotSpinner status={DOT[h.status]} size={3} label={`Helper ${HELPER_STATUS_LABEL[h.status]}`} />
            <span className="min-w-0 flex-1 truncate text-pm-text ck-chat-helper-text">Helper: {h.title}</span>
            {h.agent && <span className="shrink-0 truncate text-[10px] text-pm-muted ck-chat-helper-agent">{h.agent}</span>}
            <span className={`shrink-0 text-[10px] ${h.status === 'failed' ? 'text-pm-err' : 'text-pm-muted'} ck-chat-tool-status`}>{HELPER_STATUS_LABEL[h.status]}</span>
          </button>
        </div>
      ))}
    </div>
  );
}
