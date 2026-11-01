// What a chat session over ACP adds under its conversation on the phone: the agent's permission questions as cards
// (every option with the agent's own name, no default) and, while a turn runs, a line that says so with a Stop button.
// No settings chips, no slash menu and no hand-over here: those are for the PC.
import { useState } from 'react';
import { useAcpPhoneState } from '../../hooks/useAcpPhoneState';
import { permissionCards, runStrip } from '../../lib/acpChat';
import type { BridgeClient } from '../../lib/bridge';
import { COPY } from '../../lib/attention-copy';
import { QuestionsView, useQuestionPolling } from './QuestionsList';
import '../../styles/phone-chat.css';

export function AcpPhoneDock({
  bridge,
  paneId,
  canAnswer,
  canType,
  labels,
  questionsRevision = 0,
}: {
  bridge: BridgeClient;
  paneId: string;
  /** This phone may answer the agent's questions (`answer`). */
  canAnswer: boolean;
  /** This phone may stop the agent (`type`). Without it the line has no Stop button. */
  canType: boolean;
  labels: Map<string, string>;
  /** Bumped by the bridge `questions` event; a change reads the questions again. */
  questionsRevision?: number;
}) {
  const state = useAcpPhoneState(bridge, paneId, true);
  const { questions, refresh } = useQuestionPolling(bridge, true, questionsRevision);
  // Stop was pressed: the line reads "Stopping…" at once, before the PC's state says so.
  const [asked, setAsked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const running = !!state?.turnInFlight;
  const [wasRunning, setWasRunning] = useState(false);
  if (wasRunning !== running) {
    setWasRunning(running);
    if (!running) setAsked(false);
  }

  const cards = permissionCards(questions, paneId);
  const strip = runStrip(state, asked);
  if (cards.length === 0 && !strip) return null;

  const stop = () => {
    if (!bridge.acpCancel) return;
    setAsked(true);
    setError(null);
    bridge.acpCancel(paneId).catch((e: unknown) => {
      setAsked(false);
      setError(e instanceof Error && / -> 403/.test(e.message) ? 'This phone cannot stop the agent.' : 'Could not reach your PC.');
    });
  };
  return (
    <div className="ph-acp-dock" data-testid="acp-phone-dock">
      {cards.length > 0 && <QuestionsView bridge={bridge} questions={cards} refresh={refresh} labels={labels} bare readOnly={canAnswer ? undefined : COPY.watchOnly} />}
      {strip && (
        <div className="ph-acp-strip" data-testid="acp-phone-strip">
          <span role="status">{strip.text}</span>
          {canType && bridge.acpCancel && (
            <button type="button" className="ph-btn" onClick={stop} disabled={strip.stopping}>
              {strip.stopping ? 'Stopping…' : 'Stop'}
            </button>
          )}
        </div>
      )}
      {error && <div className="ph-error">{error}</div>}
    </div>
  );
}
