// One tool call as a row: its name and summary, and its status. Click it to open the detail (ToolDetail). A row read
// off the screen has no detail to show: only the note that its output is not read.
import { useState } from 'react';
import type { ToolStep } from '../../lib/sessionChatModel';
import { ToolDetail } from './ToolDetail';

export const STATUS_LABEL: Record<ToolStep['status'], string> = {
  running: 'running',
  ok: 'ok',
  error: 'error',
};

export interface ToolRowProps {
  step: ToolStep;
  /** The row is read off the terminal's screen: no name, no status, no detail. */
  fromScreen: boolean;
  /** Start open (Verbose). A click then closes or opens it. */
  defaultOpen?: boolean;
}

export function ToolRow({ step, fromScreen, defaultOpen = false }: ToolRowProps) {
  const [toggled, setToggled] = useState<boolean | null>(null);
  const open = toggled ?? defaultOpen;
  const { call, status } = step;
  const label = fromScreen ? null : STATUS_LABEL[status];
  return (
    <div className="ck-chat-row">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setToggled(!open)}
        className="flex w-full min-w-0 items-center gap-2 rounded px-1.5 py-0.5 text-left hover:bg-pm-raised ck-chat-tool"
      >
        {call.name && <span className="shrink-0 font-mono text-pm-text ck-chat-tool-name">{call.name}</span>}
        <span className="min-w-0 flex-1 truncate font-mono text-pm-muted ck-chat-tool-summary" title={call.summary}>
          {call.summary}
        </span>
        {label && (
          <span className={`shrink-0 text-[10px] ${status === 'error' ? 'text-pm-err' : 'text-pm-muted'} ck-chat-tool-status${status === 'error' ? ' ck-chat-tool-status--error' : ''}`}>
            {label}
          </span>
        )}
      </button>
      {open &&
        (fromScreen ? (
          <pre className="mt-0.5 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-pm-border bg-pm-panel p-1.5 font-mono text-[11px] text-pm-muted ck-chat-output">
            Tool output is not read from the screen.
          </pre>
        ) : (
          <ToolDetail step={step} />
        ))}
    </div>
  );
}
