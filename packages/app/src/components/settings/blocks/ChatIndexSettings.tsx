// Settings > General: the search index of your chats (Ctrl+Shift+F). A switch to keep it or not, what it holds,
// and a button that clears it. Clearing asks first and never touches the chats themselves.
import { useEffect, useState } from 'react';
import { chatIndexClear, chatIndexSetEnabled, chatIndexStatus, indexSizeText, type ChatIndexStatus } from '../../../lib/chatIndex';
import type { SettingsEnv } from '../model';
import { Btn, SettingRow, SettingToggle } from '../settings-ui';

export const CHAT_INDEX_CLEAR_BODY = 'This removes the saved search text. Chats themselves are not touched.';

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function ChatIndexSettings({ env }: { env: SettingsEnv }) {
  const [status, setStatus] = useState<ChatIndexStatus | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    void chatIndexStatus()
      .then((next) => live && setStatus(next))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);

  // The switch shows what the app holds (on by default, until the first answer).
  const enabled = status?.enabled ?? true;

  const toggle = async (value: boolean) => {
    env.setError(null);
    try {
      setStatus(await chatIndexSetEnabled(value));
      env.markSaved();
    } catch (error) {
      env.setError(errorText(error));
    }
  };

  const clear = async () => {
    env.setError(null);
    const ok = await env.confirm({ title: 'Clear search index?', body: CHAT_INDEX_CLEAR_BODY, confirmLabel: 'Clear' });
    if (!ok) return;
    setBusy(true);
    try {
      setStatus(await chatIndexClear());
      env.markSaved();
    } catch (error) {
      env.setError(errorText(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <SettingToggle
        label="Keep a search index of my chats"
        description="Lets Ctrl+Shift+F find what was said in the chats you have opened. The saved text is redacted and stays on this computer. Off: nothing new is saved and nothing is searched."
        checked={enabled}
        onChange={(value) => void toggle(value)}
      />
      <SettingRow label="Clear search index" description={status ? indexSizeText(status) : 'Loading…'}>
        <Btn disabled={busy || !status} onClick={() => void clear()}>
          {busy ? 'Clearing…' : 'Clear search index'}
        </Btn>
      </SettingRow>
    </>
  );
}
