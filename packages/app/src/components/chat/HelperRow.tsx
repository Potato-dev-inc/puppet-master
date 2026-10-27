// A helper agent's row ("Helper: {description}") with a small dot face. Open, it shows the prompt it was given and the
// result it sent back. Its own tool calls are not listed here: the helper's own chat is a separate pane.
import { useState } from 'react';
import type { ToolStep } from '../../lib/sessionChatModel';
import { DotSpinner } from '../cockpit/DotSpinner';
import { CodeBlock, OutputBlock } from './ToolDetail';
import { STATUS_LABEL } from './ToolRow';

export interface HelperRowProps {
  step: ToolStep;
  fromScreen: boolean;
  defaultOpen?: boolean;
}

/** The helper's own words: its `description` field, else the row's summary. */
export function helperDescription(step: ToolStep): string {
  const input = step.call.input;
  const description = input && typeof input === 'object' ? (input as Record<string, unknown>).description : undefined;
  return typeof description === 'string' && description.trim() !== '' ? description.trim() : step.call.summary;
}

export function HelperRow({ step, fromScreen, defaultOpen = false }: HelperRowProps) {
  const [toggled, setToggled] = useState<boolean | null>(null);
  const open = toggled ?? defaultOpen;
  const { call, status } = step;
  const description = helperDescription(step);
  const input = call.input && typeof call.input === 'object' ? (call.input as Record<string, unknown>) : null;
  const prompt = typeof input?.prompt === 'string' && input.prompt.trim() !== '' ? input.prompt : null;
  const running = status === 'running';
  return (
    <div className="ml-1 border-l border-pm-border pl-2 ck-chat-helper" data-testid="helper-row">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setToggled(!open)}
        className="flex w-full min-w-0 items-center gap-2 rounded px-1 py-0.5 text-left hover:bg-pm-raised ck-chat-tool ck-chat-helper-row"
      >
        <DotSpinner status={running ? 'running' : 'idle'} size={3} label={running ? 'Helper working' : 'Helper'} />
        <span className="min-w-0 flex-1 truncate text-pm-text ck-chat-helper-text" title={description}>
          Helper: {description}
        </span>
        {!fromScreen && (
          <span className={`shrink-0 text-[10px] ${status === 'error' ? 'text-pm-err' : 'text-pm-muted'} ck-chat-tool-status`}>
            {STATUS_LABEL[status]}
          </span>
        )}
      </button>
      {open && (
        <div className="mt-0.5 space-y-1 pb-1 ck-chat-detail">
          {prompt !== null && <CodeBlock label="Prompt" text={prompt} />}
          {fromScreen ? <p className="text-[11px] text-pm-muted">Tool output is not read from the screen.</p> : <OutputBlock result={step.result} />}
        </div>
      )}
    </div>
  );
}
