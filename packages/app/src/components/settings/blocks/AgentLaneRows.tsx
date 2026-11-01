// Settings > Agents > How to run: for each agent that has an enabled chat (ACP) definition, a popup that picks how a new
// session runs by default, "Terminal" (the default) or "Chat (ACP)". The terminal stays a full way to run every agent.
// The whole block is hidden while the Labs switch is off. An agent that cannot run as a chat shows why instead.
import { useCallback, useEffect, useState } from 'react';
import {
  addableChat,
  errorSentence,
  normalizeLaneView,
  tauriAcpChatApi,
  terminalChatCapable,
  TERMINAL_CHAT_LANE,
  TERMINAL_CHAT_WORD,
  type AcpAddable,
  type AcpChatApi,
  type AcpLaneAgent,
  type AcpLaneView,
  type RunLaneWord,
} from '../../../lib/acpChat';
import { Popup } from '../controls';
import { Btn, SettingRow } from '../settings-ui';
import { AddChatDialog } from './AddChatDialog';
import { RemoveChatDialog } from './RemoveChatDialog';

/** What Remove chat needs of a row: an ACP agent, or an agent that only has Chat (terminal). */
type RemovableAgent = Pick<AcpLaneAgent, 'id' | 'label' | 'addon'> & { overlay?: boolean };

export const TERMINAL_LABEL = 'Terminal';
export const CHAT_LABEL = 'Chat (ACP)';
export const ADD_CHAT_LABEL = 'Add chat (ACP)';

export function AgentLaneRows({ api = tauriAcpChatApi }: { api?: Pick<AcpChatApi, 'laneGet' | 'laneSet'> & Pick<AcpChatApi, 'overlayDetect' | 'overlaySet' | 'overlayRemove' | 'chatRemove'> }) {
  const [view, setView] = useState<AcpLaneView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState<AcpAddable | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [removing, setRemoving] = useState<RemovableAgent | null>(null);
  const [removeBusy, setRemoveBusy] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);

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
    async (id: string, lane: RunLaneWord) => {
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

  // Remove chat: the sheet asks first. Every agent that has a chat can be removed; the backend stops its open chats,
  // goes back to Terminal, drops an added chat and, if asked, deletes the add-on files.
  const confirmRemove = useCallback(
    async (agent: RemovableAgent, removeAddon: boolean) => {
      setRemoveBusy(true);
      setRemoveError(null);
      try {
        let next: AcpLaneView;
        if (api.chatRemove) next = normalizeLaneView(await api.chatRemove(agent.id, { removeAddon }));
        else if (api.overlayRemove && agent.overlay) {
          await api.overlayRemove(agent.id);
          next = normalizeLaneView(await api.laneGet());
        } else throw new Error('This version of the app cannot remove a chat.');
        setView(next);
        const failed = (next as AcpLaneView & { removal?: { addon_error?: string | null } }).removal?.addon_error;
        setStatus(failed ? `Chat removed for ${agent.label}. The add-on could not be deleted: ${failed}` : `Chat removed for ${agent.label}.`);
        setRemoving(null);
      } catch (e) {
        setRemoveError(errorSentence(e));
      } finally {
        setRemoveBusy(false);
      }
    },
    [api],
  );

  const addable = addableChat(view);
  const terminalChat = terminalChatCapable(view);
  if (!view || !view.labs_enabled || (view.agents.length === 0 && addable.length === 0 && terminalChat.length === 0)) return null;
  const terminalChatNote = (id: string) => {
    const note = terminalChat.find((t) => t.id === id)?.note;
    return note ? <span data-testid="agent-terminal-chat-note"> {note}</span> : null;
  };
  // The rows are direct children of the Settings row wrapper (a wrapping element would make Settings hide the whole block:
  // see the `.st-rowwrap:not(:has(> .st-row))` rule in settings.css). The empty marker is for tests.
  return (
    <>
      <span hidden data-testid="agent-lane" />
      {view.agents.map((agent) => (
        <SettingRow
          key={agent.id}
          label={agent.label}
          description={
            <>
              {agent.disabled ? <span data-testid="agent-lane-note">{agent.disabled_note ?? 'Chat is not available for this agent.'}</span> : 'New sessions start in a terminal unless you pick Chat.'}
              {terminalChatNote(agent.id)}
            </>
          }
        >
          <Btn
            small
            aria-label={`Remove chat for ${agent.label}`}
            onClick={() => {
              setRemoveError(null);
              setRemoving(agent);
            }}
          >
            Remove chat
          </Btn>
          {!agent.disabled && (
            <Popup aria-label={`How ${agent.label} runs`} value={agent.lane} onChange={(event) => void save(agent.id, event.target.value as RunLaneWord)}>
              <option value="terminal">{TERMINAL_LABEL}</option>
              <option value="acp">{CHAT_LABEL}</option>
              {terminalChat.some((t) => t.id === agent.id) && <option value={TERMINAL_CHAT_LANE}>{TERMINAL_CHAT_WORD}</option>}
            </Popup>
          )}
        </SettingRow>
      ))}
      {terminalChat
        .filter((t) => !view.agents.some((a) => a.id === t.id))
        .map((t) => (
          <SettingRow
            key={`tc-${t.id}`}
            label={t.label}
            description={<>New sessions start in a terminal unless you pick {TERMINAL_CHAT_WORD}.{terminalChatNote(t.id)}</>}
          >
            {t.chosen && (
              <Btn
                small
                aria-label={`Remove chat for ${t.label}`}
                onClick={() => {
                  setRemoveError(null);
                  setRemoving({ id: t.id, label: t.label });
                }}
              >
                Remove chat
              </Btn>
            )}
            <Popup aria-label={`How ${t.label} runs`} value={t.chosen ? TERMINAL_CHAT_LANE : 'terminal'} onChange={(event) => void save(t.id, event.target.value as RunLaneWord)}>
              <option value="terminal">{TERMINAL_LABEL}</option>
              <option value={TERMINAL_CHAT_LANE}>{TERMINAL_CHAT_WORD}</option>
            </Popup>
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
      {removing && (
        <RemoveChatDialog
          agent={removing}
          busy={removeBusy}
          error={removeError}
          onCancel={() => setRemoving(null)}
          onRemove={(removeAddon) => void confirmRemove(removing, removeAddon)}
        />
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
    </>
  );
}
