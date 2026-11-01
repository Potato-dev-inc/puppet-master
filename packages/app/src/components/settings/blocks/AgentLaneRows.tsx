// Settings > Agents > How to run: for each agent that has an enabled chat (ACP) definition, a popup that picks how a new
// session runs by default, "Terminal" (the default) or "Chat (ACP)". The terminal stays a full way to run every agent.
// The whole block is hidden while the Labs switch is off. An agent that cannot run as a chat shows why instead.
import { useCallback, useEffect, useState } from 'react';
import { addableChat, errorSentence, normalizeLaneView, tauriAcpChatApi, type AcpAddable, type AcpChatApi, type AcpLane, type AcpLaneView } from '../../../lib/acpChat';
import { Popup } from '../controls';
import { Btn, SettingRow } from '../settings-ui';
import { AddChatDialog } from './AddChatDialog';

export const TERMINAL_LABEL = 'Terminal';
export const CHAT_LABEL = 'Chat (ACP)';
export const ADD_CHAT_LABEL = 'Add chat (ACP)';

export function AgentLaneRows({ api = tauriAcpChatApi }: { api?: Pick<AcpChatApi, 'laneGet' | 'laneSet'> & Pick<AcpChatApi, 'overlayDetect' | 'overlaySet' | 'overlayRemove'> }) {
  const [view, setView] = useState<AcpLaneView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState<AcpAddable | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .laneGet()
      .then((v) => alive && setView(normalizeLaneView(v)))
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

  const refresh = useCallback(async () => {
    try {
      setView(normalizeLaneView(await api.laneGet()));
    } catch {
      // the old list stays
    }
  }, [api]);

  const remove = useCallback(
    async (id: string, label: string) => {
      setError(null);
      setStatus(null);
      try {
        if (!api.overlayRemove) throw new Error('This version of the app cannot remove a chat.');
        await api.overlayRemove(id);
        setStatus(`Chat removed for ${label}.`);
        await refresh();
      } catch (e) {
        setError(errorSentence(e));
      }
    },
    [api, refresh],
  );

  const addable = addableChat(view);
  if (!view || !view.labs_enabled || (view.agents.length === 0 && addable.length === 0)) return null;
  return (
    <div data-testid="agent-lane">
      {view.agents.map((agent) => (
        <SettingRow
          key={agent.id}
          label={agent.label}
          description={agent.disabled ? <span data-testid="agent-lane-note">{agent.disabled_note ?? 'Chat is not available for this agent.'}</span> : 'New sessions start in a terminal unless you pick Chat.'}
        >
          {agent.overlay && (
            <Btn small aria-label={`Remove chat for ${agent.label}`} onClick={() => void remove(agent.id, agent.label)}>
              Remove chat
            </Btn>
          )}
          {!agent.disabled && (
            <Popup aria-label={`How ${agent.label} runs`} value={agent.lane} onChange={(event) => void save(agent.id, event.target.value as AcpLane)}>
              <option value="terminal">{TERMINAL_LABEL}</option>
              <option value="acp">{CHAT_LABEL}</option>
            </Popup>
          )}
        </SettingRow>
      ))}
      {addable.map((a) => (
        <SettingRow
          key={`add-${a.id}`}
          label={a.label}
          description={a.reason ? <span data-testid="agent-add-chat-note">{a.reason}</span> : 'No chat yet. Add one to run this agent as a chat.'}
        >
          <Btn small primary={!a.reason} disabled={!!a.reason} aria-label={`${ADD_CHAT_LABEL} for ${a.label}`} onClick={() => setAdding(a)}>
            {ADD_CHAT_LABEL}
          </Btn>
        </SettingRow>
      ))}
      {status && (
        <p role="status" className="text-xs text-pm-muted">
          {status}
        </p>
      )}
      {adding && (
        <AddChatDialog
          agent={adding}
          api={api}
          onClose={() => setAdding(null)}
          onAdded={() => {
            setStatus(`Chat added for ${adding.label}.`);
            setAdding(null);
            void refresh();
          }}
        />
      )}
      {error && (
        <p role="alert" className="text-xs text-pm-text">
          {error}
        </p>
      )}
    </div>
  );
}
