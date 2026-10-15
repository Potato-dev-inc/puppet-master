// The confirm sheet for "Install" in Set up agents. It never runs anything itself: it shows the exact
// command, where it comes from, and what to expect, then asks. The command shown is the command the app
// checks again when Run is pressed, so what you read is what runs.
import { useState } from 'react';
import { sourceLine, type InstallPlan } from '../../lib/agent-install';
import { Sheet } from './Sheet';

export function InstallSheet({
  plan,
  busy,
  error,
  onRun,
  onCancel,
  onCopy,
}: {
  plan: InstallPlan;
  busy: boolean;
  error: string | null;
  /** `acknowledgedRisk` is true only after the second confirmation of a downloaded script. */
  onRun: (acknowledgedRisk: boolean) => void;
  onCancel: () => void;
  onCopy: (text: string) => void;
}) {
  const [trusted, setTrusted] = useState(false);
  const step = plan.steps[0];

  if (!plan.available || !step) {
    return (
      <Sheet labelledBy="ck-install-t" onClose={onCancel}>
        <h2 className="ck-section" id="ck-install-t">
          Install {plan.label}
        </h2>
        <p className="ck-sub">{plan.reason ?? 'Puppet Master cannot install this one from here.'}</p>
        {plan.hint && <code className="ck-doctor-cmd">{plan.hint}</code>}
        <div className="ck-sheet-actions">
          {plan.hint && (
            <button type="button" className="ck-btn ck-btn--plain" onClick={() => onCopy(plan.hint!)}>
              Copy command
            </button>
          )}
          <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={onCancel}>
            Close
          </button>
        </div>
      </Sheet>
    );
  }

  const later = plan.steps.slice(1);
  const needsTrust = step.high_risk;
  const canRun = !busy && (!needsTrust || trusted);
  return (
    <Sheet labelledBy="ck-install-t" onClose={busy ? undefined : onCancel} className="ck-sheet--install">
      <h2 className="ck-section" id="ck-install-t">
        Install {step.label}
      </h2>
      {later.length > 0 && (
        <p className="ck-sub">
          {plan.label} needs {step.label} first. This is step 1 of {plan.steps.length}.
        </p>
      )}
      <p className="ck-sub">Puppet Master will run this in a window you can watch:</p>
      <code className="ck-doctor-cmd" data-testid="install-command">
        {step.command}
      </code>
      <p className="ck-sub">{sourceLine(step)}</p>
      {step.method === 'winget' && (
        <p className="ck-sub">Windows may ask if the installer can make changes. That is expected.</p>
      )}
      {step.note && <p className="ck-sub">{step.note}</p>}
      {later.length > 0 && (
        <p className="ck-sub">
          Next: <code className="ck-doctor-inline">{later.map((s) => s.command).join('  then  ')}</code>
        </p>
      )}
      {needsTrust && (
        <label className="ck-install-risk">
          <input type="checkbox" checked={trusted} onChange={(e) => setTrusted(e.target.checked)} />
          <span>
            This downloads a script from the internet and runs it. I trust {step.source_host}.
          </span>
        </label>
      )}
      {error && <p className="ck-sub ck-doctor-error">{error}</p>}
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--plain" onClick={() => onCopy(step.command)}>
          Copy command
        </button>
        <button type="button" className="ck-btn ck-btn--secondary" autoFocus disabled={busy} onClick={onCancel}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" disabled={!canRun} onClick={() => onRun(trusted)}>
          {busy ? 'Starting…' : 'Run'}
        </button>
      </div>
    </Sheet>
  );
}
