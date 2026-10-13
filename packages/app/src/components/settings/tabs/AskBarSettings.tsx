// Settings > General: the Ask bar (global shortcut that asks an agent a quick question).
import { useEffect, useState } from 'react';
import { ASKBAR_SHORTCUTS, tauriAskbarApi, type AskbarAgent, type AskbarApi, type AskbarSettings } from '../../../lib/askbar';
import { FieldSelect, SettingBlock, SettingToggle } from '../settings-ui';

export function AskBarSettingsBlock({ api = tauriAskbarApi }: { api?: AskbarApi }) {
  const [s, setS] = useState<AskbarSettings | null>(null);
  const [agents, setAgents] = useState<AskbarAgent[]>([]);
  useEffect(() => {
    api.settingsGet().then(setS).catch(() => {});
    api.agents().then(setAgents).catch(() => {});
  }, [api]);
  if (!s) return null;
  const save = (next: AskbarSettings) => {
    setS(next);
    api.settingsSet(next).then(setS).catch(() => {});
  };
  return (
    <>
      <SettingToggle
        label="Ask bar"
        description="Open a small bar from anywhere in Windows to ask an agent a quick question. Off by default."
        checked={s.enabled}
        onChange={(enabled) => save({ ...s, enabled })}
        implemented
      />
      <SettingBlock label="Ask bar shortcut" implemented description="Pick the keys that open the bar.">
        <FieldSelect value={s.shortcut} disabled={!s.enabled} onChange={(e) => save({ ...s, shortcut: e.target.value })}>
          {ASKBAR_SHORTCUTS.map((key) => (
            <option key={key} value={key}>
              {key}
            </option>
          ))}
        </FieldSelect>
      </SettingBlock>
      <SettingBlock label="Ask bar agent" implemented>
        <FieldSelect
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
      </SettingBlock>
      <SettingToggle
        label="Attach clipboard by default"
        description="Off by default. When on, each open reads the clipboard and attaches it to your question. Clipboard text can hold passwords or keys."
        checked={s.attach_clipboard_default}
        onChange={(attach_clipboard_default) => save({ ...s, attach_clipboard_default })}
        implemented
      />
    </>
  );
}
