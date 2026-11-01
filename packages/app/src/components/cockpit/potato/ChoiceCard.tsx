// The choice card under a new bot's first message: a stack of outlined buttons. A click sends that choice's text as the
// person's first message (through `onPick`, the thread's normal send path). The buttons wait while one is being sent.
import { useState } from 'react';
import type { FirstRunChoice } from '../../../lib/potato-firstrun';
import '../../../styles/cockpit-potato-firstrun.css';

export interface ChoiceCardProps {
  choices: readonly FirstRunChoice[];
  /** Sends the text of the chosen button. Resolves when the send has been asked for. */
  onPick: (text: string) => Promise<void>;
}

export function ChoiceCard({ choices, onPick }: ChoiceCardProps) {
  const [busy, setBusy] = useState(false);

  const pick = async (choice: FirstRunChoice) => {
    if (busy) return;
    setBusy(true);
    try {
      await onPick(choice.text);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ck-potato-choice" role="group" aria-label="Where to start">
      {choices.map((choice) => (
        <button key={choice.id} type="button" className="ck-btn ck-btn--secondary ck-potato-choice-btn" disabled={busy} onClick={() => void pick(choice)}>
          {choice.label}
        </button>
      ))}
    </div>
  );
}
