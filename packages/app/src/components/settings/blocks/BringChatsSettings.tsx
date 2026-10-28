// Settings > General: "Bring my old chats". Shows how many old chats are in search, a button that opens the Bring
// panel, and a button that removes them (asks first). Reading this status touches no agent folder.
import { useEffect, useState } from 'react';
import { chatCount, chatImportClear, chatImportState, requestBringChats } from '../../../lib/chatImport';
import type { SettingsEnv } from '../model';
import { Btn, SettingRow } from '../settings-ui';

export const BRING_CHATS_REMOVE_BODY = "This removes the old chats Puppet Master added to search. Your agents' own chat files are not touched.";

/** "Not added" for none, otherwise "538 chats" (or "1 chat"). */
export function bringChatsStatusText(chats: number): string {
  if (chats <= 0) return 'Not added';
  return chatCount(chats);
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function BringChatsSettings({ env }: { env: SettingsEnv }) {
  // null until the first answer; the count only, never a folder.
  const [chats, setChats] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    void chatImportState()
      .then((state) => live && setChats(state.imported.chats))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);

  const remove = async () => {
    env.setError(null);
    const ok = await env.confirm({ title: 'Remove imported chats?', body: BRING_CHATS_REMOVE_BODY, confirmLabel: 'Remove' });
    if (!ok) return;
    setBusy(true);
    try {
      const status = await chatImportClear();
      setChats(status.importedChats ?? 0);
      env.markSaved();
    } catch (error) {
      env.setError(errorText(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingRow label="Bring my old chats" description={chats === null ? 'Loading…' : bringChatsStatusText(chats)}>
      <Btn onClick={() => requestBringChats()}>Add or update</Btn>
      <Btn disabled={busy || !chats} onClick={() => void remove()}>
        {busy ? 'Removing…' : 'Remove imported chats'}
      </Btn>
    </SettingRow>
  );
}
