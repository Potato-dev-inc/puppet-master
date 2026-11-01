// "Let agents answer these questions": off by default. Off, only the person can answer the agent's permission questions;
// on, an agent that started this chat through Puppet Master (or any agent tool) may answer them too.
import { useEffect, useState } from 'react';
import { errorSentence, tauriAcpChatApi, type AcpChatApi, type AcpChatState } from '../../lib/acpChat';
import '../../styles/cockpit-acp.css';

export const AGENT_ANSWERING_LABEL = 'Let agents answer these questions';
export const AGENT_ANSWERING_OFF = 'Off: only you can answer.';
export const AGENT_ANSWERING_ON = 'On: an agent that started this chat may answer its permission questions.';

export function AgentAnsweringSwitch({ paneId, state, api = tauriAcpChatApi }: { paneId: string; state: AcpChatState | null; api?: Pick<AcpChatApi, 'setAgentAnswering'> }) {
  // The pick shows at once; the backend's state replaces it, and a refusal takes it back.
  const [pending, setPending] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const confirmed = state?.agentAnswering ?? false;
  useEffect(() => setPending(null), [confirmed, paneId]);

  if (!state) return null;
  const on = pending ?? confirmed;
  const toggle = () => {
    const next = !on;
    setError(null);
    setPending(next);
    api.setAgentAnswering(paneId, next).catch((e: unknown) => {
      setPending(null);
      setError(errorSentence(e));
    });
  };
  return (
    <div className="ck-acp-answering" data-testid="agent-answering">
      <div className="ck-menu-toggle">
        <span id={`ck-answering-${paneId}`}>{AGENT_ANSWERING_LABEL}</span>
        <button type="button" role="switch" aria-checked={on} aria-labelledby={`ck-answering-${paneId}`} className="ck-switch" onClick={toggle}>
          <span className="ck-switch-knob" />
        </button>
      </div>
      <p className="ck-acp-answering-note">{on ? AGENT_ANSWERING_ON : AGENT_ANSWERING_OFF}</p>
      {error && (
        <p className="ck-acp-answering-note" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
