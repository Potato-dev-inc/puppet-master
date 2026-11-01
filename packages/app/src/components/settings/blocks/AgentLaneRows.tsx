// Settings > Agents > How to run: for each agent that has an enabled chat (ACP) definition, a popup that picks how a new
// session runs by default, "Terminal" (the default) or "Chat (ACP)". The terminal stays a full way to run every agent.
// The whole block is hidden while the Labs switch is off. An agent that cannot run as a chat shows why instead.
import { useCallback, useEffect, useState } from 'react';
import { errorSentence, tauriAcpChatApi, type AcpChatApi, type AcpLane, type AcpLaneView } from '../../../lib/acpChat';
import { Popup } from '../controls';
import { SettingRow } from '../settings-ui';

export const TERMINAL_LABEL = 'Terminal';
export const CHAT_LABEL = 'Chat (ACP)';

export function AgentLaneRows({ api = tauriAcpChatApi }: { api?: Pick<AcpChatApi, 'laneGet' | 'laneSet'> }) {
  const [view, setView] = useState<AcpLaneView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .laneGet()
      .then((v) => alive && setView(v))
      // No desktop shell (a browser preview): there is nothing to choose, so show nothing.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [api]);

  const save = useCallback(
    async (id: string, lane: AcpLane) => {
      setError(null);
      try {
        setView(await api.laneSet(id, lane));
      } catch (e) {
        setError(errorSentence(e));
      }
    },
    [api],
  );

  if (!view || !view.labs_enabled || view.agents.length === 0) return null;
  return (
    <div data-testid="agent-lane">
      {view.agents.map((agent) => (
        <SettingRow
          key={agent.id}
          label={agent.label}
          description={agent.disabled ? <span data-testid="agent-lane-note">{agent.disabled_note ?? 'Chat is not available for this agent.'}</span> : 'New sessions start in a terminal unless you pick Chat.'}
        >
          {!agent.disabled && (
            <Popup aria-label={`How ${agent.label} runs`} value={agent.lane} onChange={(event) => void save(agent.id, event.target.value as AcpLane)}>
              <option value="terminal">{TERMINAL_LABEL}</option>
              <option value="acp">{CHAT_LABEL}</option>
            </Popup>
          )}
        </SettingRow>
      ))}
      {error && (
        <p role="alert" className="text-xs text-pm-text">
          {error}
        </p>
      )}
    </div>
  );
}
