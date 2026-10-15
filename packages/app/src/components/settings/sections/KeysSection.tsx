// Settings > Keys: provider API keys, OpenCode A/B key profiles, rate-limit key automation, and removing keys.
// Secrets save on blur / Enter (never per keystroke, never on a Save button). OpenCode keys and the
// automation settings save through their own Tauri commands and tick the row when they succeed.
import { useCallback, useEffect, useState } from 'react';
import type { SettingsPatch } from '../../../lib/settings';
import { secretsApi, type SecretName } from '../../../lib/secrets';
import { tauri, type OpenCodeKeyStatus } from '../../../lib/tauri';
import { ApiKeyField } from '../ApiKeyField';
import { SecretControl } from '../controls';
import type { RowDef, SettingsEnv } from '../model';
import { Btn, FieldLabel, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';

type AutomationMode = 'auto_if_backup' | 'notify_only';

const KEYWORDS = 'api key anthropic openai openrouter opencode rotate rate limit keychain credential';

const OPENCODE_HINT =
  'For OpenCode workers only. Kept in the system keychain when one is available. Never sent to the phone or to other tools.';
/** The opt-in's name (founder decision 2026-10-15). Off by default; a person's earlier explicit choice is kept by the backend. */
const AUTOMATION_LABEL = "Switch to another of your keys when one hits its rate limit (check your provider's terms)";
const AUTOMATION_HINT = 'When this is off, Puppet Master only tells you a key needs swapping. Needs a second OpenCode key (B).';
const RESTART_HINT = 'Restart the OpenCode pane after a key switch so it picks up the new key.';
const REMOVE_ALL_HINT =
  'Deletes the Anthropic, OpenAI and OpenRouter keys from this computer. Agents lose access until you add keys again. OpenCode keys are not removed.';

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// ---------- OpenCode key status (each row keeps its own copy) ----------

function useOpenCodeKeyStatus() {
  const [status, setStatus] = useState<OpenCodeKeyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      setError(null);
      setStatus(await tauri.getOpenCodeKeyStatus());
    } catch (e) {
      setError(errorText(e));
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  return { status, setStatus, error, refresh };
}

// ---------- provider keys ----------

interface ProviderKeyDef {
  id: string;
  label: string;
  name: SecretName;
  placeholder: string;
  hint: string;
}

const PROVIDER_KEYS: ProviderKeyDef[] = [
  {
    id: 'key-anthropic',
    label: 'Anthropic API key',
    name: 'anthropic_api_key',
    placeholder: 'sk-ant-…',
    hint: 'Used by agents on Anthropic models. Kept in your system keychain when one is available.',
  },
  {
    id: 'key-openai',
    label: 'OpenAI API key',
    name: 'openai_api_key',
    placeholder: 'sk-…',
    hint: 'Used by agents on OpenAI models. Kept in your system keychain when one is available.',
  },
  {
    id: 'key-openrouter',
    label: 'OpenRouter API key',
    name: 'openrouter_api_key',
    placeholder: 'sk-or-…',
    hint: 'Used by agents that route through OpenRouter. Kept in your system keychain when one is available.',
  },
];

function ProviderKey({ env, def }: { env: SettingsEnv; def: ProviderKeyDef }) {
  return (
    <SettingBlock label={def.label} description={def.hint}>
      <ApiKeyField
        label={def.label}
        name={def.name}
        placeholder={def.placeholder}
        fileValue={env.settings[def.name]}
        onFileChange={(value) => {
          const change: SettingsPatch = {};
          change[def.name] = value ?? null;
          return env.patch(change);
        }}
        onSaved={env.markSaved}
        onConfirmRemove={() =>
          env.confirm({
            title: `Remove the ${def.label.replace(' API key', '')} key?`,
            body: 'Agents that use it lose access until you add a key again.',
            confirmLabel: 'Remove',
          })
        }
      />
    </SettingBlock>
  );
}

// ---------- OpenCode A / B keys ----------

function OpenCodeKeys({ env }: { env: SettingsEnv }) {
  const { status, error, refresh } = useOpenCodeKeyStatus();

  const profileLine = (id: 'a' | 'b') => {
    if (!status) return 'Checking…';
    const profile = status.profiles.find((entry) => entry.id === id);
    if (!profile) return 'Unknown';
    if (!profile.configured) return 'Not set';
    return status.active_profile === id ? 'Configured - active' : 'Configured';
  };

  const save = async (id: 'a' | 'b', secret: string) => {
    env.setError(null);
    try {
      await tauri.setOpenCodeKeyProfile(id, secret, id === 'a' ? 'Primary' : 'Secondary');
      await refresh();
      env.markSaved();
    } catch (e) {
      env.setError(`Not saved: ${errorText(e)}`);
    }
  };

  return (
    <SettingBlock label="OpenCode API keys (A / B)" description={OPENCODE_HINT}>
      <div className="space-y-3">
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-1">
            <FieldLabel>Key A - {profileLine('a')}</FieldLabel>
            <SecretControl aria-label="OpenCode key A" placeholder="Leave blank to keep existing" onCommit={(secret) => save('a', secret)} />
          </div>
          <div className="space-y-1">
            <FieldLabel>Key B - {profileLine('b')}</FieldLabel>
            <SecretControl aria-label="OpenCode key B" placeholder="Leave blank to keep existing" onCommit={(secret) => save('b', secret)} />
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Btn onClick={() => void refresh()}>Refresh status</Btn>
        </div>
        {error && <Note role="alert">{`Could not read OpenCode key status: ${error}`}</Note>}
      </div>
    </SettingBlock>
  );
}

// ---------- key automation ----------

function AutomationMode({ env }: { env: SettingsEnv }) {
  const { status, setStatus, error } = useOpenCodeKeyStatus();
  const [busy, setBusy] = useState(false);

  const change = async (on: boolean) => {
    setBusy(true);
    env.setError(null);
    try {
      setStatus(await tauri.setOpenCodeAutomationSettings(on ? 'auto_if_backup' : 'notify_only', status?.restart_pane_on_rotate ?? true));
      env.markSaved();
    } catch (e) {
      env.setError(`Not saved: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <SettingToggle
        label={AUTOMATION_LABEL}
        description={AUTOMATION_HINT}
        checked={status?.key_automation_mode === 'auto_if_backup'}
        disabled={!status || busy}
        onChange={(value) => void change(value)}
      />
      {error && <Note role="alert">{`Could not read key status: ${error}`}</Note>}
    </>
  );
}

function RestartPane({ env }: { env: SettingsEnv }) {
  const { status, setStatus } = useOpenCodeKeyStatus();
  const [busy, setBusy] = useState(false);

  const change = async (restart: boolean) => {
    setBusy(true);
    env.setError(null);
    const mode: AutomationMode = status?.key_automation_mode === 'auto_if_backup' ? 'auto_if_backup' : 'notify_only';
    try {
      setStatus(await tauri.setOpenCodeAutomationSettings(mode, restart));
      env.markSaved();
    } catch (e) {
      env.setError(`Not saved: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingToggle
      label="Restart pane after key rotate"
      description={RESTART_HINT}
      checked={status?.restart_pane_on_rotate ?? true}
      disabled={!status || busy}
      onChange={(value) => void change(value)}
    />
  );
}

// ---------- remove all ----------

function RemoveAllKeys({ env }: { env: SettingsEnv }) {
  const [busy, setBusy] = useState(false);

  const run = async () => {
    env.setError(null);
    const ok = await env.confirm({
      title: 'Remove all keys?',
      body: 'Deletes the Anthropic, OpenAI and OpenRouter keys from this computer. Agents that use them lose access until you add keys again. OpenCode keys are not removed.',
      confirmLabel: 'Remove all',
    });
    if (!ok) return;
    setBusy(true);
    try {
      if ((await secretsApi.storageInfo()).secure) {
        for (const def of PROVIDER_KEYS) await secretsApi.remove(def.name);
      }
      await env.patch({ anthropic_api_key: null, openai_api_key: null, openrouter_api_key: null });
      env.markSaved();
    } catch (e) {
      env.setError(`Not removed: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingRow label="Remove all keys" description={REMOVE_ALL_HINT}>
      <Btn disabled={busy} onClick={() => void run()}>
        Remove…
      </Btn>
    </SettingRow>
  );
}

// ---------- the rows ----------

export const keysRows: RowDef[] = [
  ...PROVIDER_KEYS.map(
    (def): RowDef => ({
      id: def.id,
      label: def.label,
      hint: def.hint,
      keywords: KEYWORDS,
      group: 'Provider keys',
      render: (env) => <ProviderKey env={env} def={def} />,
    }),
  ),
  {
    id: 'opencode-keys',
    label: 'OpenCode API keys (A / B)',
    hint: OPENCODE_HINT,
    keywords: `${KEYWORDS} OpenCode key A OpenCode key B refresh status`,
    group: 'OpenCode keys',
    render: (env) => <OpenCodeKeys env={env} />,
  },
  {
    id: 'opencode-automation',
    labs: { name: 'Key switching', about: 'Move to your spare OpenCode key when one hits its rate limit.' },
    label: AUTOMATION_LABEL,
    hint: AUTOMATION_HINT,
    keywords: `${KEYWORDS} automation key automation mode switch backup second key terms`,
    group: 'OpenCode keys',
    render: (env) => <AutomationMode env={env} />,
  },
  {
    id: 'opencode-restart',
    label: 'Restart pane after key rotate',
    hint: RESTART_HINT,
    keywords: `${KEYWORDS} restart pane rotate switch`,
    group: 'OpenCode keys',
    render: (env) => <RestartPane env={env} />,
  },
  {
    id: 'remove-all-keys',
    label: 'Remove all keys',
    hint: REMOVE_ALL_HINT,
    keywords: `${KEYWORDS} remove delete clear`,
    group: 'Remove keys',
    danger: true,
    render: (env) => <RemoveAllKeys env={env} />,
  },
];
