// A helper request in the bot's thread (spec section 2.2): "Fish wants a helper: …", the folder, the agent, and three
// buttons. Nothing starts without Start. The answers go through `onAnswer`; until the helper piece lands the real
// transport says "Coming soon." and the card shows it.
import { useState } from 'react';
import { plainError, type HelperAnswer } from '../../../lib/potato';

export interface HelperCardProps {
  name: string;
  task: string;
  /** The folder name the bot asked for, if it said one. */
  project?: string;
  /** The label of the agent that would do the work. */
  agentLabel: string;
  /** False once somebody answered: the buttons are gone. */
  open: boolean;
  onAnswer: (answer: HelperAnswer) => Promise<void>;
}

export function HelperCard({ name, task, project, agentLabel, open, onAnswer }: HelperCardProps) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const answer = async (choice: HelperAnswer) => {
    if (busy) return;
    setBusy(true);
    setProblem(null);
    try {
      await onAnswer(choice);
    } catch (error) {
      setProblem(plainError(error, 'That did not work. Try again.'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className={`ck-potato-card${open ? '' : ' is-closed'}`} aria-label={`${name} wants a helper`}>
      <p className="ck-potato-card-title">
        {name} wants a helper: <span className="ck-potato-card-task">{task}</span>
      </p>
      <p className="ck-sub ck-potato-card-fact">
        Folder: <span className="ck-mono">{project || 'not chosen yet'}</span>
      </p>
      <p className="ck-sub ck-potato-card-fact">Agent: {agentLabel}</p>
      {open && (
        <div className="ck-potato-card-actions">
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void answer('start')}>
            Start
          </button>
          <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => void answer('change_folder')}>
            Change folder
          </button>
          <button type="button" className="ck-btn ck-btn--plain" disabled={busy} onClick={() => void answer('not_now')}>
            Not now
          </button>
        </div>
      )}
      {problem && (
        <p className="ck-sub ck-potato-card-problem" role="status">
          {problem}
        </p>
      )}
    </section>
  );
}
