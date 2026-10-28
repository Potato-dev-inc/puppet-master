// The line that stands for a run of tool rows in a turn: "Read 6 files · Ran 2 commands · Edited 3 files · 48s".
// Click it to open the rows. While the turn runs, it shows a live spinner and the running row's label instead.
import { useState } from 'react';
import type { ToolStep } from '../../lib/sessionChatModel';
import { DotSpinner } from '../cockpit/DotSpinner';
import '../../styles/cockpit-chat-activity.css';
import { activitySpanMs, activitySummary, activitySummaryParts, runningLabel, type SummaryPart } from './chatActivity';
import { formatDuration } from './chatFormat';
import { ToolRow } from './ToolRow';

export interface ActivityLineProps {
  steps: ToolStep[];
  /** The turn has no end yet: a row without a result is then live. */
  live: boolean;
  fromScreen: boolean;
  /** Verbose: the line and its rows start open. */
  verbose: boolean;
}

/** The summary's groups as text, with each "(N failed)" in a stronger style, then the time when there is one. */
function SummaryText({ parts, time }: { parts: SummaryPart[]; time: string | null }) {
  return (
    <>
      {parts.map((part, i) => (
        <span key={i}>
          {i > 0 && ' · '}
          {part.text}
          {part.failed !== null && (
            <span className="ck-chat-activity-failed">
              {' '}
              {part.failed}
            </span>
          )}
        </span>
      ))}
      {time !== null && ` · ${time}`}
    </>
  );
}

export function ActivityLine({ steps, live, fromScreen, verbose }: ActivityLineProps) {
  const [toggled, setToggled] = useState<boolean | null>(null);
  const open = toggled ?? verbose;
  const running = live ? runningLabel(steps) : null;
  const summary = activitySummary(steps);
  const span = live ? null : activitySpanMs(steps);
  const parts = activitySummaryParts(steps);
  return (
    <div className="ck-chat-activity-wrap">
      <button
        type="button"
        aria-expanded={open}
        title={summary}
        onClick={() => setToggled(!open)}
        className="flex w-full min-w-0 items-center gap-2 rounded px-1.5 py-1 text-left text-pm-muted hover:bg-pm-raised ck-chat-activity"
        data-live={running !== null || undefined}
      >
        {running !== null && <DotSpinner status="running" size={3} label="Working" className="shrink-0" />}
        <span className="min-w-0 flex-1 truncate text-[12px] ck-chat-activity-text">
          {running !== null ? `Working · ${running}` : <SummaryText parts={parts} time={span !== null ? formatDuration(span) : null} />}
        </span>
        <span aria-hidden="true" className={`shrink-0 text-[10px] ck-chat-chevron${open ? ' ck-chat-chevron--open' : ''}`} />
      </button>
      {open && (
        <div className="mt-0.5 ml-2 space-y-0.5 border-l border-pm-border pl-1.5 ck-chat-activity-rows">
          {steps.map((step) => (
            <ToolRow key={step.id} step={step} fromScreen={fromScreen} defaultOpen={verbose} />
          ))}
        </div>
      )}
    </div>
  );
}
