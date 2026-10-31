// Settings > Potato Bot (spec section 5). It is shown only while Labs has Potato Bot on. The switch here is the kill
// switch (the same flag as General > Labs). Everything else is saved by Tauri commands in
// `<app data>/potato/settings.json`, never in the settings file the phone can patch. The DeepSeek key goes to the
// system keychain through the same key field as the other keys; this page only ever sees "set" and the last 4.
import { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { brainLabel, dollars, plainError, tauriPotatoTransport, type BrainKind, type PotatoSettings, type PotatoTransport, type TalkPersona } from '../../../lib/potato';
import { secretsApi, type SecretsApi } from '../../../lib/secrets';
import { ApiKeyField } from '../ApiKeyField';
import { NumberControl } from '../controls';
import type { RowDef } from '../model';
import { Btn, FieldSelect, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';

/** The limits of the daily limit, in cents ($0.05 to $20). Rust clamps again. */
export const MIN_LIMIT_CENTS = 5;
export const MAX_LIMIT_CENTS = 2000;

export const BRAIN_NOTE: Record<BrainKind, string> = {
  deepseek: 'Pays DeepSeek per message. Usually cents a day.',
  go: 'Uses your OpenCode Go sign-in.',
  agents: 'No extra cost.',
};
const BRAINS: BrainKind[] = ['deepseek', 'go', 'agents'];
const PERSONAS: TalkPersona[] = ['fish', 'potato'];

// ---- the saved settings, read once and shared by the rows -------------------------------------------------------

interface Store {
  value: PotatoSettings | null;
  failed: string | null;
  listeners: Set<() => void>;
}
const stores = new WeakMap<PotatoTransport, Store>();

function storeOf(transport: PotatoTransport): Store {
  let store = stores.get(transport);
  if (!store) {
    store = { value: null, failed: null, listeners: new Set() };
    stores.set(transport, store);
  }
  return store;
}

function usePotatoSettings(transport: PotatoTransport) {
  const store = storeOf(transport);
  const value = useSyncExternalStore(
    (listener) => {
      store.listeners.add(listener);
      return () => store.listeners.delete(listener);
    },
    () => store.value,
  );
  const tell = () => store.listeners.forEach((l) => l());
  useEffect(() => {
    let live = true;
    transport
      .settingsGet()
      .then((found) => {
        if (!live) return;
        store.value = found;
        store.failed = null;
        tell();
      })
      .catch((error) => {
        if (!live) return;
        store.failed = plainError(error, 'Could not read the Potato Bot settings.');
        tell();
      });
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [transport]);
  const save = useCallback(
    async (change: Partial<PotatoSettings>) => {
      const base = store.value ?? (await transport.settingsGet());
      const saved = await transport.settingsSet({ ...base, ...change });
      store.value = saved;
      store.failed = null;
      store.listeners.forEach((l) => l());
      return saved;
    },
    [store, transport],
  );
  return { settings: value, failed: store.failed, save };
}

// ---- the rows -------------------------------------------------------------------------------------------------------

function BrainRow({ transport, onSaved, setError }: { transport: PotatoTransport; onSaved: () => void; setError: (m: string | null) => void }) {
  const { settings, save } = usePotatoSettings(transport);
  const brain = settings?.brain ?? 'deepseek';
  return (
    <SettingRow label="Talk brain" description={BRAIN_NOTE[brain]}>
      <FieldSelect
        aria-label="Talk brain"
        value={brain}
        disabled={!settings}
        onChange={(e) => {
          setError(null);
          save({ brain: e.target.value as BrainKind }).then(onSaved, (error) => setError(plainError(error, 'Could not save that.')));
        }}
      >
        {BRAINS.map((kind) => (
          <option key={kind} value={kind}>
            {brainLabel(kind)}
          </option>
        ))}
      </FieldSelect>
    </SettingRow>
  );
}

function KeyRow({ transport, api }: { transport: PotatoTransport; api: SecretsApi }) {
  const { settings } = usePotatoSettings(transport);
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const check = async () => {
    setChecking(true);
    setResult(null);
    try {
      setResult({ ok: true, text: await transport.checkKey() });
    } catch (error) {
      setResult({ ok: false, text: plainError(error, 'Could not check the key.') });
    } finally {
      setChecking(false);
    }
  };
  if (settings && settings.brain !== 'deepseek') {
    return <SettingRow label="DeepSeek key" description={settings.brain === 'go' ? 'Not needed: this brain uses your OpenCode Go sign-in.' : 'Not needed: this brain uses your own agents.'} />;
  }
  return (
    <SettingBlock label="DeepSeek key" description="Kept in your system keychain. Only the last 4 characters are ever shown here, and the key never goes to the phone.">
      <ApiKeyField
        label="DeepSeek key"
        name="deepseek_api_key"
        placeholder="sk-…"
        api={api}
        onSaved={() => setResult(null)}
        onFileChange={() => Promise.reject(new Error('Potato Bot keeps its key only in the system keychain, and this computer has none.'))}
      />
      <div className="st-key-check">
        <Btn disabled={checking} onClick={() => void check()}>
          {checking ? 'Checking…' : 'Check key'}
        </Btn>
        {result && (
          <Note role={result.ok ? 'status' : 'alert'}>{result.text}</Note>
        )}
      </div>
    </SettingBlock>
  );
}

function PersonaRow({ transport, onSaved, setError }: { transport: PotatoTransport; onSaved: () => void; setError: (m: string | null) => void }) {
  const { settings, save } = usePotatoSettings(transport);
  return (
    <SettingRow label="Default persona" description="The voice a new bot starts with. You can change a bot's voice any time in its chat.">
      <FieldSelect
        aria-label="Default persona"
        value={settings?.defaultPersona ?? 'fish'}
        disabled={!settings}
        onChange={(e) => {
          setError(null);
          save({ defaultPersona: e.target.value as TalkPersona }).then(onSaved, (error) => setError(plainError(error, 'Could not save that.')));
        }}
      >
        {PERSONAS.map((pack) => (
          <option key={pack} value={pack}>
            {pack === 'fish' ? 'Fish' : 'Potato'}
          </option>
        ))}
      </FieldSelect>
    </SettingRow>
  );
}

/** Dollars typed in the box, kept between $0.05 and $20 and to whole cents. */
export function limitCentsOf(dollarsTyped: number): number {
  return Math.min(MAX_LIMIT_CENTS, Math.max(MIN_LIMIT_CENTS, Math.round(dollarsTyped * 100)));
}

function LimitRow({ transport, onSaved, setError }: { transport: PotatoTransport; onSaved: () => void; setError: (m: string | null) => void }) {
  const { settings, save } = usePotatoSettings(transport);
  return (
    <SettingRow
      label="Daily limit"
      description={`The most one bot may spend in a day, on every chat. From ${dollars(MIN_LIMIT_CENTS)} to ${dollars(MAX_LIMIT_CENTS)}. At the limit it stops until tomorrow.`}
    >
      <NumberControl
        aria-label="Daily limit in dollars"
        value={(settings?.dailyLimitCents ?? 50) / 100}
        min={MIN_LIMIT_CENTS / 100}
        max={MAX_LIMIT_CENTS / 100}
        step={0.05}
        normalize={(n) => limitCentsOf(n) / 100}
        suffix="dollars a day"
        disabled={!settings}
        onCommit={(next) => {
          setError(null);
          save({ dailyLimitCents: limitCentsOf(next) }).then(onSaved, (error) => setError(plainError(error, 'Could not save that.')));
        }}
      />
    </SettingRow>
  );
}

const KEYWORDS = 'potato fish bot chat friend brain deepseek opencode go agents persona limit budget cost';

export function makePotatoRows(transport: PotatoTransport = tauriPotatoTransport, api: SecretsApi = secretsApi): RowDef[] {
  return [
    {
      id: 'potato-bot',
      label: 'Potato Bot',
      hint: 'The on and off switch. Off stops every bot brain now and takes back any helper request.',
      keywords: `${KEYWORDS} kill switch off on stop`,
      group: 'Potato Bot',
      render: (env) => (
        <SettingToggle
          label="Potato Bot"
          description="On: your bots can chat and ask you to start helpers. Off: every brain stops now and open helper requests are taken back. Turn it back on in General › Labs."
          checked={env.settings.potato_bot_enabled ?? false}
          onChange={(value) => void env.patch({ potato_bot_enabled: value })}
        />
      ),
    },
    {
      id: 'potato-brain',
      label: 'Talk brain',
      hint: 'Which AI answers when you chat: DeepSeek, OpenCode Go, or your own agents.',
      keywords: KEYWORDS,
      group: 'Potato Bot',
      render: (env) => <BrainRow transport={transport} onSaved={env.markSaved} setError={env.setError} />,
    },
    {
      id: 'potato-key',
      label: 'DeepSeek key',
      hint: 'The key Potato Bot uses with DeepSeek. Check key sends one tiny message to see that it works.',
      keywords: `${KEYWORDS} api key check test keychain`,
      group: 'Potato Bot',
      render: () => <KeyRow transport={transport} api={api} />,
    },
    {
      id: 'potato-persona',
      label: 'Default persona',
      hint: 'The voice a new bot starts with, Fish or Potato.',
      keywords: KEYWORDS,
      group: 'Potato Bot',
      render: (env) => <PersonaRow transport={transport} onSaved={env.markSaved} setError={env.setError} />,
    },
    {
      id: 'potato-limit',
      label: 'Daily limit',
      hint: 'The most one bot may spend in a day. From $0.05 to $20.',
      keywords: `${KEYWORDS} dollars money spend`,
      group: 'Potato Bot',
      render: (env) => <LimitRow transport={transport} onSaved={env.markSaved} setError={env.setError} />,
    },
  ];
}

export const potatoRows: RowDef[] = makePotatoRows();
