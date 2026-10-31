// Why a bot cannot answer, in one card above the message box (spec section 4): needs a key, needs Node, the daily
// limit, stopped, or the last error. A bot that can answer has no card.
import { useState } from 'react';
import { COPY, errorSentence, plainError, type BrainState } from '../../../lib/potato';

export interface BrainStateCardProps {
  name: string;
  state: BrainState;
  lastError: string | null;
  limitCents: number;
  onStart: () => Promise<void>;
  onInstallNode: () => Promise<void>;
  onOpenSettings: () => void;
}

export function BrainStateCard({ name, state, lastError, limitCents, onStart, onInstallNode, onOpenSettings }: BrainStateCardProps) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const run = async (call: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setProblem(null);
    try {
      await call();
    } catch (error) {
      setProblem(plainError(error, 'That did not work. Try again.'));
    } finally {
      setBusy(false);
    }
  };

  let text: string | null = null;
  let action: { label: string; run: () => void } | null = null;
  switch (state) {
    case 'needs_key':
      text = COPY.noKey(name);
      action = { label: 'Open Settings', run: onOpenSettings };
      break;
    case 'needs_node':
      text = COPY.needsNode;
      action = { label: 'Install Node.js', run: () => void run(onInstallNode) };
      break;
    case 'daily_limit':
      text = COPY.limit(name, limitCents);
      action = { label: 'Open Settings', run: onOpenSettings };
      break;
    case 'stopped':
      text = COPY.stopped(name);
      action = { label: 'Start again', run: () => void run(onStart) };
      break;
    case 'missing_program':
      text = COPY.missingProgram;
      break;
    case 'ready':
    case 'idle':
    case 'restarting':
      text = errorSentence(name, lastError);
      break;
    default:
      break;
  }
  if (!text) return null;

  return (
    <div className="ck-potato-state" role="status">
      <p className="ck-potato-state-text">{text}</p>
      {action && (
        <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" disabled={busy} onClick={action.run}>
          {action.label}
        </button>
      )}
      {problem && <p className="ck-sub ck-potato-state-problem">{problem}</p>}
    </div>
  );
}
