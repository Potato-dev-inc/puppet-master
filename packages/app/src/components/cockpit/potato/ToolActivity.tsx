// One line over a run of the bot's tool use ("Used the computer · 3 steps", "Started a helper"). It starts folded. Opened,
// it lists each step as a compact row: a tick or an alert, what it did in words, a short extra. A raw result (JSON) is
// never shown in the row; it sits behind that step's "Details".
import { useId, useState } from 'react';
import { THREAD_COPY, activityTitle, type ToolStep } from '../../../lib/potato-thread';
import { ThreadIcon } from './ThreadIcons';
import { StatusPill } from './ui/StatusPill';
import '../../../styles/cockpit-potato-thread.css';

export function ToolActivity({ steps }: { steps: ToolStep[] }) {
  const [open, setOpen] = useState(false);
  const listId = useId();
  const needsYou = steps.some((s) => s.needsYou);
  return (
    <li className="ck-potato-activity">
      <button type="button" className="ck-potato-activity-head" aria-expanded={open} aria-controls={listId} onClick={() => setOpen((v) => !v)}>
        <span className="ck-potato-chevron" data-open={open || undefined}>
          <ThreadIcon name="chevron" size={12} />
        </span>
        <span className="ck-potato-activity-title">{activityTitle(steps)}</span>
        {needsYou && <StatusPill tone="strong">{THREAD_COPY.needsYou}</StatusPill>}
      </button>
      {open && (
        <ul id={listId} className="ck-potato-steps" aria-label="Steps">
          {steps.map((step) => (
            <StepRow key={step.id} step={step} />
          ))}
        </ul>
      )}
    </li>
  );
}

function StepRow({ step }: { step: ToolStep }) {
  const [showDetails, setShowDetails] = useState(false);
  const detailsId = useId();
  return (
    <li className={`ck-potato-step${step.ok ? '' : ' is-failed'}`}>
      <span className="ck-potato-step-mark" role="img" aria-label={step.ok ? 'Done' : THREAD_COPY.failed}>
        <ThreadIcon name={step.ok ? 'check' : 'alert'} size={13} />
      </span>
      <span className="ck-potato-step-text">
        <span className="ck-potato-step-label">{step.label}</span>
        {step.summary && <span className="ck-potato-step-sum">{step.summary}</span>}
        {step.needsYou && <StatusPill tone="strong">{THREAD_COPY.needsYou}</StatusPill>}
      </span>
      {step.details && (
        <button type="button" className="ck-potato-step-details" aria-expanded={showDetails} aria-controls={detailsId} onClick={() => setShowDetails((v) => !v)}>
          {showDetails ? THREAD_COPY.hideDetails : THREAD_COPY.details}
        </button>
      )}
      {step.details && showDetails && (
        <pre id={detailsId} className="ck-potato-step-raw">
          {step.details}
        </pre>
      )}
    </li>
  );
}
