// Settings > API keys: provider keys, OpenCode A/B key profiles and rate-limit key automation.
import { useEffect, useState } from 'react';
import { tauri, type OpenCodeKeyStatus } from '../../../lib/tauri';
import { FieldInput, FieldLabel, FieldSelect, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import type { SettingsTabContext } from './tab-context';

export function ApiTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings } = ctx;
  const [keyStatus, setKeyStatus] = useState<OpenCodeKeyStatus | null>(null);
  const [keyA, setKeyA] = useState('');
  const [keyB, setKeyB] = useState('');
  const [keyError, setKeyError] = useState<string | null>(null);
  const [keySaving, setKeySaving] = useState(false);
  const [automationSaving, setAutomationSaving] = useState(false);

  const refreshKeyStatus = async () => {
    try {
      setKeyError(null);
      setKeyStatus(await tauri.getOpenCodeKeyStatus());
    } catch (err) {
      setKeyError(err instanceof Error ? err.message : String(err));
    }
  };

  useEffect(() => {
    void refreshKeyStatus();
  }, []);

  const saveOpenCodeKeys = async () => {
    setKeySaving(true);
    setKeyError(null);
    try {
      if (keyA.trim()) {
        await tauri.setOpenCodeKeyProfile('a', keyA.trim(), 'Primary');
      }
      if (keyB.trim()) {
        await tauri.setOpenCodeKeyProfile('b', keyB.trim(), 'Secondary');
      }
      setKeyA('');
      setKeyB('');
      await refreshKeyStatus();
    } catch (err) {
      setKeyError(err instanceof Error ? err.message : String(err));
    } finally {
      setKeySaving(false);
    }
  };

  const saveAutomation = async (mode: 'auto_if_backup' | 'notify_only', restart: boolean) => {
    setAutomationSaving(true);
    setKeyError(null);
    try {
      setKeyStatus(await tauri.setOpenCodeAutomationSettings(mode, restart));
    } catch (err) {
      setKeyError(err instanceof Error ? err.message : String(err));
    } finally {
      setAutomationSaving(false);
    }
  };

  const profileLine = (id: string) => {
    const profile = keyStatus?.profiles.find((entry) => entry.id === id);
    if (!profile) return 'Unknown';
    const active = keyStatus?.active_profile === id ? ' · active' : '';
    return profile.configured ? `Configured${active}` : 'Not set';
  };

  return (
    <SettingsSection title="API keys" description="Keys are stored in secure OS storage in the desktop build.">
      <SettingBlock label="Anthropic API key" implemented><FieldInput type="password" value={settings.anthropic_api_key ?? ''} onChange={(e) => setSettings({ ...settings, anthropic_api_key: e.target.value })} className="font-mono" placeholder="sk-ant-…" autoComplete="off" /></SettingBlock>
      <SettingBlock label="OpenAI API key" implemented><FieldInput type="password" value={settings.openai_api_key ?? ''} onChange={(e) => setSettings({ ...settings, openai_api_key: e.target.value })} className="font-mono" placeholder="sk-…" autoComplete="off" /></SettingBlock>
      <SettingBlock label="OpenRouter API key" implemented><FieldInput type="password" value={settings.openrouter_api_key ?? ''} onChange={(e) => setSettings({ ...settings, openrouter_api_key: e.target.value })} className="font-mono" placeholder="sk-or-…" autoComplete="off" /></SettingBlock>
      <SettingBlock
        label="OpenCode API keys (A / B)"
        implemented
        description="Stored locally for opencode_native workers only — never synced to mobile or returned by MCP. Orchestrators can call rotate_opencode_key to switch profiles when rate-limited."
      >
        <div className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <FieldLabel>Key A — {profileLine('a')}</FieldLabel>
              <FieldInput type="password" value={keyA} onChange={(e) => setKeyA(e.target.value)} className="font-mono" placeholder="Leave blank to keep existing" autoComplete="off" />
            </div>
            <div>
              <FieldLabel>Key B — {profileLine('b')}</FieldLabel>
              <FieldInput type="password" value={keyB} onChange={(e) => setKeyB(e.target.value)} className="font-mono" placeholder="Leave blank to keep existing" autoComplete="off" />
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void saveOpenCodeKeys()}
              disabled={keySaving || (!keyA.trim() && !keyB.trim())}
              className="rounded-lg border border-pm-accent/50 bg-pm-accent/10 px-3 py-2 text-sm font-semibold text-pm-accent hover:bg-pm-accent/20 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {keySaving ? 'Saving…' : 'Save OpenCode keys'}
            </button>
            <button type="button" onClick={() => void refreshKeyStatus()} className="rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">
              Refresh status
            </button>
          </div>
          {keyError && (
            <p className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs leading-5 text-red-300">
              {keyError}
            </p>
          )}
        </div>
      </SettingBlock>
      <SettingBlock
        label="OpenCode key automation"
        implemented
        description="When an opencode_native worker hits API rate limits: auto-rotate to the backup key if configured, or only notify the orchestrator."
      >
        <div className="space-y-3">
          <FieldLabel>Automation mode</FieldLabel>
          <FieldSelect
            value={keyStatus?.key_automation_mode ?? 'auto_if_backup'}
            disabled={automationSaving || !keyStatus}
            onChange={(e) => {
              const mode = e.target.value as 'auto_if_backup' | 'notify_only';
              void saveAutomation(mode, keyStatus?.restart_pane_on_rotate ?? true);
            }}
          >
            <option value="auto_if_backup">Auto-rotate if backup key configured</option>
            <option value="notify_only">Notify only (no auto-rotate)</option>
          </FieldSelect>
          <SettingToggle
            label="Restart pane after key rotate"
            description="Restart the opencode_native pane after an automatic rotation so it picks up the new key."
            checked={keyStatus?.restart_pane_on_rotate ?? true}
            implemented={!automationSaving && !!keyStatus}
            onChange={(checked) => {
              const mode = (keyStatus?.key_automation_mode ?? 'auto_if_backup') as 'auto_if_backup' | 'notify_only';
              void saveAutomation(mode, checked);
            }}
          />
        </div>
      </SettingBlock>
      <SettingBlock label="GitHub token" implemented={false}><FieldInput type="password" className="font-mono" placeholder="ghp_…" disabled /></SettingBlock>
      <SettingBlock label="Linear API key" implemented={false}><FieldInput type="password" className="font-mono" placeholder="lin_api_…" disabled /></SettingBlock>
      <SettingToggle label="Validate keys on save" description="Probe provider APIs with a minimal request before storing." checked onChange={() => undefined} implemented={false} />
    </SettingsSection>
  );
}
