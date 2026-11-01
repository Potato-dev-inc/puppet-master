// The agent's own settings (model, mode, effort) as a row of small selects above the message box of a chat session. One
// select per setting the agent offers; the row is not drawn when it offers none. Settings change between turns, so the
// selects are switched off while the agent works (and when the conversation is open in a terminal).
import { useEffect, useState } from 'react';
import { chipsDisabled, configChips, errorSentence, tauriAcpChatApi, type AcpChatApi, type AcpChatState } from '../../lib/acpChat';
import '../../styles/cockpit-acp.css';

export function AcpConfigChips({ paneId, state, api = tauriAcpChatApi }: { paneId: string; state: AcpChatState | null; api?: Pick<AcpChatApi, 'setConfig'> }) {
  // A pick shows at once; the backend's state replaces it when it arrives, and a refusal takes it back.
  const [picked, setPicked] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setPicked({});
    setError(null);
  }, [state?.configOptions, paneId]);

  const chips = configChips(state);
  if (chips.length === 0) return null;
  const off = chipsDisabled(state);
  const change = (id: string, value: string) => {
    setError(null);
    setPicked((cur) => ({ ...cur, [id]: value }));
    api.setConfig(paneId, id, value).catch((e: unknown) => {
      setPicked((cur) => {
        const { [id]: _gone, ...rest } = cur;
        return rest;
      });
      setError(errorSentence(e));
    });
  };
  return (
    <div className="ck-acp-chips" data-testid="acp-chips">
      <div className="ck-acp-chips-row" role="group" aria-label="Chat settings">
        {chips.map((chip) => (
          <select
            key={chip.id}
            className="ck-acp-chip"
            aria-label={chip.label}
            title={chip.label}
            value={picked[chip.id] ?? chip.value}
            disabled={off}
            onChange={(event) => change(chip.id, event.target.value)}
          >
            {chip.choices.map((choice) => (
              <option key={choice.value} value={choice.value} title={choice.description ?? undefined}>
                {choice.name}
              </option>
            ))}
          </select>
        ))}
      </div>
      {error && (
        <p className="ck-acp-chips-note" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
