// Settings > General > Ask bar: the global shortcut that opens a small bar to ask an agent a quick question.
// It saves through its own commands (`askbar_settings_get/set`), then flashes the row's "Saved" tick.
import { useEffect, useState } from 'react';
import { ASKBAR_SHORTCUTS, tauriAskbarApi, type AskbarAgent, type AskbarApi, type AskbarSettings } from '../../../lib/askbar';
import { FieldSelect, SettingRow, SettingToggle } from '../settings-ui';

export function AskBarSettingsBlock({ api = tauriAskbarApi, onSaved }: { api?: AskbarApi; onSaved?: () => void }) {
  const [s, setS] = useState<AskbarSettings | null>(null);
  const [agents, setAgents] = useState<AskbarAgent[]>([]);
  useEffect(() => {
    api.settingsGet().then(setS).catch(() => {});
    api.agents().then(setAgents).catch(() => {});
  }, [api]);
  if (!s) return null;
  const save = (next: AskbarSettings) => {
    setS(next);
    api
      .settingsSet(next)
      .then((saved) => {
        setS(saved);
        onSaved?.();
      })
      .catch(() => {});
  };
  return (
    <>
      <SettingToggle
        label="Ask bar"
        description="Open a small bar from anywhere in Windows to ask an agent a quick question. Off by default."
        checked={s.enabled}
        onChange={(enabled) => save({ ...s, enabled })}
      />
      <SettingRow label="Ask bar shortcut" description="Pick the keys that open the bar.">
        <FieldSelect aria-label="Ask bar shortcut" value={s.shortcut} disabled={!s.enabled} onChange={(e) => save({ ...s, shortcut: e.target.value })}>
          {ASKBAR_SHORTCUTS.map((key) => (
            <option key={key} value={key}>
              {key}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>
      <SettingRow label="Ask bar agent" description="Which agent answers. Last used keeps the one you picked last.">
        <FieldSelect
          aria-label="Ask bar agent"
          value={s.default_agent ?? ''}
          disabled={!s.enabled}
          onChange={(e) => save({ ...s, default_agent: e.target.value || null })}
        >
          <option value="">Last used</option>
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.label}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>
      <SettingToggle
        label="Attach clipboard by default"
        description="Off by default. When on, each open reads the clipboard and attaches it to your question. Clipboard text can hold passwords or keys."
        checked={s.attach_clipboard_default}
        onChange={(attach_clipboard_default) => save({ ...s, attach_clipboard_default })}
      />
    </>
  );
}
