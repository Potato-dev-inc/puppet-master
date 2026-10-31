// Settings > Phone > Telegram > Tag Your Agent in groups. Someone tags the bot in a group the person approved
// ("@mybot is this true?") and the person's own agent answers in the thread. It is read-only: nothing is changed,
// no pane is touched. Off until the person turns it on. A group is approved only here, on the PC.
import { useCallback, useEffect, useState } from 'react';
import {
  onTelegramChanged,
  telegramTagApprove,
  telegramTagBot,
  telegramTagIgnore,
  telegramTagRemove,
  telegramTagSetCaps,
  telegramTagSetEnabled,
  telegramTagUpdateGroup,
  type TagAnswer,
  type TagBotView,
  type TagGroup,
  type TagGroupUpdate,
  type TagWho,
} from '../../../lib/telegram-api';
import { NumberControl, TextControl } from '../controls';
import type { SettingsEnv } from '../model';
import { Btn, FieldSelect, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';

export const TAG_TITLE = 'Tag Your Agent in groups';
export const TAG_DESCRIPTION =
  'People in a group you approve can tag your bot with a question. Your own agent answers in the thread. It only reads: it never changes files.';
export const TAG_PRIVACY = 'Keep Telegram privacy mode on — the bot then only sees messages that tag it.';
export const TAG_ANYONE_FOLDER_WARNING =
  'Anyone in this group can make your agent read this folder and post what it finds. Leave the folder empty unless you trust the group.';
export const TAG_REMOVE_BODY = 'The bot goes quiet in this group at once. You can approve it again later.';
/** The most recent answers listed. */
export const RECENT_SHOWN = 5;

const WHO_OPTIONS: { value: TagWho; label: string }[] = [
  { value: 'only_me', label: 'Only me' },
  { value: 'anyone', label: 'Anyone in the group' },
];

/** The seam the block talks through: the Tauri wrappers in production, a fake in tests. */
export interface TagBotApi {
  view: () => Promise<TagBotView>;
  setEnabled: (on: boolean) => Promise<TagBotView>;
  setCaps: (dailyCap: number, perPersonCap: number) => Promise<TagBotView>;
  approve: (chatId: number) => Promise<TagBotView>;
  ignore: (chatId: number) => Promise<TagBotView>;
  updateGroup: (chatId: number, update: TagGroupUpdate) => Promise<TagBotView>;
  remove: (chatId: number) => Promise<TagBotView>;
  onChanged: (listener: () => void) => () => void;
}

export const tagBotApi: TagBotApi = {
  view: telegramTagBot,
  setEnabled: telegramTagSetEnabled,
  setCaps: telegramTagSetCaps,
  approve: telegramTagApprove,
  ignore: telegramTagIgnore,
  updateGroup: telegramTagUpdateGroup,
  remove: telegramTagRemove,
  onChanged: (listener) => {
    const pending = onTelegramChanged(listener);
    return () => void pending.then((unlisten) => unlisten());
  },
};

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/** The update that keeps a group as it is. */
const asUpdate = (g: TagGroup): TagGroupUpdate => ({ who: g.who, agent: g.agent, folder: g.folder, style: g.style });

/** One line for the list of recent answers. */
export function answerLine(a: TagAnswer): string {
  return a.ok && a.answer ? a.answer : "Couldn't answer.";
}

function GroupCard({
  group,
  view,
  busy,
  onUpdate,
  onRemove,
}: {
  group: TagGroup;
  view: TagBotView;
  busy: boolean;
  onUpdate: (update: TagGroupUpdate) => void;
  onRemove: () => void;
}) {
  // An agent the group already uses stays in the list even if it is not installed right now.
  const agents = view.agents.some((a) => a.id === group.agent)
    ? view.agents
    : group.agent
      ? [...view.agents, { id: group.agent, label: group.agent }]
      : view.agents;
  return (
    <div className="st-tag-group" role="group" aria-label={group.title}>
      <div className="st-tag-head">
        <strong>{group.title}</strong>
        <Btn small disabled={busy} aria-label={`Remove ${group.title}`} onClick={onRemove}>
          Remove
        </Btn>
      </div>
      <SettingRow label="Who may tag">
        <FieldSelect
          aria-label={`Who may tag in ${group.title}`}
          value={group.who}
          disabled={busy}
          onChange={(e) => onUpdate({ ...asUpdate(group), who: e.target.value as TagWho })}
        >
          {WHO_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>
      <SettingRow label="Agent">
        <FieldSelect
          aria-label={`Agent for ${group.title}`}
          value={group.agent ?? ''}
          disabled={busy}
          onChange={(e) => onUpdate({ ...asUpdate(group), agent: e.target.value || null })}
        >
          <option value="">Any agent that can answer</option>
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.label}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>
      <SettingRow label="Folder (optional)" description="A project folder the agent may read. Empty: it sees no project files.">
        <TextControl
          mode="blur"
          aria-label={`Folder for ${group.title}`}
          placeholder="Empty"
          value={group.folder ?? ''}
          disabled={busy}
          normalize={(t) => t.trim()}
          onCommit={(next) => onUpdate({ ...asUpdate(group), folder: next || null })}
        />
      </SettingRow>
      {group.who === 'anyone' && group.folder && <Note role="alert">{TAG_ANYONE_FOLDER_WARNING}</Note>}
      <SettingRow label="Style note (optional)" description="How the bot should sound here. 200 characters at most.">
        <TextControl
          mode="blur"
          aria-label={`Style note for ${group.title}`}
          placeholder="For example: short and friendly"
          maxLength={200}
          value={group.style}
          disabled={busy}
          normalize={(t) => t.trim()}
          onCommit={(next) => onUpdate({ ...asUpdate(group), style: next })}
        />
      </SettingRow>
    </div>
  );
}

export function TelegramTagBot({ env, api = tagBotApi }: { env: SettingsEnv; api?: TagBotApi }) {
  const [view, setView] = useState<TagBotView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    (alive: () => boolean) => {
      api
        .view()
        .then((next) => alive() && setView(next))
        .catch((err) => alive() && setError(errText(err)));
    },
    [api],
  );

  useEffect(() => {
    let live = true;
    const reload = () => load(() => live);
    reload();
    const off = api.onChanged(reload);
    return () => {
      live = false;
      off();
    };
  }, [api, load]);

  /** Runs one change and shows what came back. A refusal stays on screen as the reason. */
  const change = async (action: () => Promise<TagBotView>) => {
    setError(null);
    setBusy(true);
    try {
      setView(await action());
      env.markSaved();
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (g: TagGroup) => {
    const ok = await env.confirm({ title: `Remove ${g.title}?`, body: TAG_REMOVE_BODY, confirmLabel: 'Remove' });
    if (ok) void change(() => api.remove(g.chatId));
  };

  // The bot has to be connected first (Step 2 above); until then there is nothing to switch on.
  if (!view?.paired) return error ? <Note role="alert">{error}</Note> : null;

  return (
    <>
      {error && <Note role="alert">{error}</Note>}
      <SettingToggle
        label={TAG_TITLE}
        description={TAG_DESCRIPTION}
        checked={view.enabled}
        disabled={busy}
        onChange={(on) => void change(() => api.setEnabled(on))}
      />
      <Note>{TAG_PRIVACY}</Note>
      {view.enabled && (
        <>
          <SettingBlock
            label="Groups waiting for your yes"
            description="A group shows up here when someone tags your bot in it. The bot says nothing there until you approve."
          >
            {view.pending.length === 0 ? (
              <Note>None right now.</Note>
            ) : (
              <ul className="st-tag-list">
                {view.pending.map((p) => (
                  <li key={p.chatId}>
                    <span>{p.title}</span>
                    <span className="st-tag-actions">
                      <Btn small primary disabled={busy} aria-label={`Approve ${p.title}`} onClick={() => void change(() => api.approve(p.chatId))}>
                        Approve
                      </Btn>
                      <Btn small disabled={busy} aria-label={`Ignore ${p.title}`} onClick={() => void change(() => api.ignore(p.chatId))}>
                        Ignore
                      </Btn>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </SettingBlock>
          <SettingBlock label="Approved groups">
            {view.groups.length === 0 ? (
              <Note>No groups yet.</Note>
            ) : (
              view.groups.map((g) => (
                <GroupCard
                  key={g.chatId}
                  group={g}
                  view={view}
                  busy={busy}
                  onUpdate={(update) => void change(() => api.updateGroup(g.chatId, update))}
                  onRemove={() => void remove(g)}
                />
              ))
            )}
          </SettingBlock>
          <SettingRow label="Answers a day" description="For the whole bot. After this it says so once in each group, then stays quiet.">
            <NumberControl
              aria-label="Answers a day"
              min={1}
              max={500}
              value={view.dailyCap}
              onCommit={(next) => void change(() => api.setCaps(next, view.perPersonCap))}
            />
          </SettingRow>
          <SettingRow label="Answers an hour for one person" description="Only for groups where anyone may tag.">
            <NumberControl
              aria-label="Answers an hour for one person"
              min={1}
              max={60}
              value={view.perPersonCap}
              onCommit={(next) => void change(() => api.setCaps(view.dailyCap, next))}
            />
          </SettingRow>
          <SettingBlock label="Recent answers" description="The last questions the bot answered in your groups.">
            {view.recent.length === 0 ? (
              <Note>Nothing yet.</Note>
            ) : (
              <ul className="st-tag-recent">
                {view.recent.slice(0, RECENT_SHOWN).map((a, i) => (
                  <li key={`${a.atMs}-${i}`}>
                    <small>{`${a.groupTitle} · ${a.asker} · ${a.agent}`}</small>
                    <span className="st-tag-q">{a.question}</span>
                    <span className="st-tag-a">{answerLine(a)}</span>
                  </li>
                ))}
              </ul>
            )}
          </SettingBlock>
        </>
      )}
    </>
  );
}
