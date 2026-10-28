// The Setup card's "Agent" row (two cells of the card's list). It shows the agent the companion runs on and lets the
// person move it to another one ("Wake Mochi on Codex until 17:00": same name, face and memory, a different plan).
// A move waits while the companion is working or has a card open; the row says so and can take it back.
import { useCallback, useEffect, useState } from 'react';
import type { CompanionAgentOption, CompanionTransport, CompanionView } from '../../../lib/companion';
import { MovePicker } from './MovePicker';
import { movedNote, movesWhenFree, untilText } from './model';

export function AgentRow({
  companion,
  transport,
  onChanged,
  onNotify,
  now = Date.now,
}: {
  companion: CompanionView;
  transport: CompanionTransport;
  /** Called after a command, so the list is read again at once. */
  onChanged: () => void;
  onNotify: (text: string) => void;
  now?: () => number;
}) {
  const [agents, setAgents] = useState<CompanionAgentOption[] | null>(null);
  const [busy, setBusy] = useState(false);
  // The picker starts over after every move (a new key), so it never keeps a stale choice.
  const [round, setRound] = useState(0);

  useEffect(() => {
    let live = true;
    transport
      .agents()
      .then((list) => live && setAgents(list))
      .catch(() => live && setAgents([]));
    return () => {
      live = false;
    };
  }, [transport]);

  const send = useCallback(
    async (patch: { agentType: string; untilMs?: number | null }, failed: string) => {
      if (busy) return;
      setBusy(true);
      try {
        await transport.update(companion.id, patch);
        setRound((n) => n + 1);
        onChanged();
      } catch (err) {
        onNotify(err instanceof Error ? err.message : failed);
      } finally {
        setBusy(false);
      }
    },
    [busy, companion.id, onChanged, onNotify, transport],
  );

  const queued = companion.queuedMove;
  const note = movedNote(companion, now());
  return (
    <>
      <dt className="ck-companion-agent-label">Agent</dt>
      <dd className="ck-companion-agent">
        {agents && agents.length > 0 ? (
          <MovePicker
            key={`${companion.agentType}:${round}`}
            currentAgent={companion.agentType}
            agents={agents}
            submitLabel="Move"
            busy={busy}
            now={now}
            onMove={(agentType, untilMs) => void send({ agentType, untilMs }, 'Could not move it.')}
          />
        ) : (
          <span>{companion.agentLabel}</span>
        )}
        {queued && (
          <div className="ck-companion-move-note" role="status">
            <span>{movesWhenFree(companion.name)}</span>
            <span className="ck-sub">
              To {queued.agentLabel}
              {queued.untilMs ? `, until ${untilText(queued.untilMs, now())}` : ''}
            </span>
            <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" disabled={busy} onClick={() => void send({ agentType: companion.agentType }, 'Could not cancel the move.')}>
              Cancel move
            </button>
          </div>
        )}
        {!queued && note && (
          <div className="ck-companion-move-note">
            <span className="ck-sub">{note}</span>
            {companion.previousAgent && (
              <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" disabled={busy} onClick={() => void send({ agentType: companion.previousAgent! }, 'Could not move it back.')}>
                Move back
              </button>
            )}
          </div>
        )}
      </dd>
    </>
  );
}
