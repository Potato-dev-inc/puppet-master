// The card for a companion whose home folder is too broad (the Desktop, a drive, a folder of projects): agents that
// work anywhere below that folder read the companion's brief and think they are the companion. One action,
// "Move <name> to its own folder", runs `companion_rehome`. Nothing moves until the person presses it.
import { useState } from 'react';
import type { CompanionView } from '../../../lib/companion';
import { CardButton, InboxCard } from '../InboxCard';
import { homeProblemTitle, plainError } from './model';

export function HomeProblemCard({
  companion,
  onMove,
}: {
  companion: CompanionView;
  /** Moves it (the desktop calls `companion_rehome`). A rejection leaves the card up with the reason. */
  onMove: (id: string) => Promise<unknown> | void;
}) {
  const problem = companion.homeProblem;
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  if (!problem) return null;
  const move = () => {
    setBusy(true);
    setFailure(null);
    Promise.resolve(onMove(companion.id))
      .catch((err: unknown) => setFailure(plainError(err instanceof Error ? err.message : 'Could not move it. Nothing was changed.')))
      .finally(() => setBusy(false));
  };
  return (
    <InboxCard
      kind="companion"
      glyph="needs"
      title={homeProblemTitle(companion.name, problem.place)}
      reason={`${companion.name} and its notes will move to ${problem.suggestedFolder}. Nothing else in the old folder is touched.`}
      note={failure}
      data={{ 'data-companion-id': companion.id, 'data-home-problem': '' }}
      actions={
        <CardButton tone="primary" role="answer" disabled={busy} onClick={move}>
          {busy ? 'Moving…' : `Move ${companion.name} to its own folder`}
        </CardButton>
      }
    />
  );
}
