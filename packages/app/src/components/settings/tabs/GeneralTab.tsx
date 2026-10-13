// Settings > General: settings file location, default project folder, theme, app version and update check.
import { useEffect, useState } from 'react';
import { homeDir } from '@tauri-apps/api/path';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type { Settings } from '@puppet-master/shared';
import { resolveSettingsFilePath } from '../../../lib/settings';
import { clampAwayMinutes, clampCheckpointsKeep } from '../../../lib/bridge-settings';
import { tauri, type AppInstallInfo } from '../../../lib/tauri';
import { FieldInput, FieldSelect, InfoCard, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import type { SettingsTabContext } from './tab-context';

export function GeneralTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, projectPath, onProjectPathChange, updateCheck, updateChecking, onCheckForUpdates, onOpenRelease } = ctx;
  const [settingsPath, setSettingsPath] = useState('settings.json');
  const [installInfo, setInstallInfo] = useState<AppInstallInfo | null>(null);
  useEffect(() => { void resolveSettingsFilePath().then(setSettingsPath); }, []);
  useEffect(() => { void tauri.getAppInstallInfo().then(setInstallInfo); }, []);
  const pickPath = async () => {
    if (!onProjectPathChange) return;
    const result = await openDialog({ directory: true, multiple: false, defaultPath: (await homeDir()) ?? projectPath ?? undefined });
    if (typeof result === 'string') {
      await onProjectPathChange(result);
      setSettings({ ...settings, project_path: result });
    }
  };
  return (
    <SettingsSection title="General" description="Launch behavior, defaults, and product preferences.">
      <InfoCard title="Settings file" description={settingsPath} implemented />
      <SettingBlock label="Default project folder" implemented description="Workspace root for new panes and orchestrator tools.">
        <div className="flex gap-2">
          <FieldInput value={projectPath ?? settings.project_path ?? ''} onChange={(e) => setSettings({ ...settings, project_path: e.target.value })} className="font-mono" placeholder="~/work/my-project" />
          <button type="button" onClick={() => void pickPath()} className="shrink-0 rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">Browse</button>
        </div>
      </SettingBlock>
      <SettingBlock label="Theme" implemented description="Desktop chrome theme (stored in settings.json).">
        <FieldSelect
          value={settings.theme ?? 'dark'}
          onChange={(e) => setSettings({ ...settings, theme: e.target.value as Settings['theme'] })}
        >
          <option value="dark">Dark</option>
          <option value="light">Light</option>
        </FieldSelect>
      </SettingBlock>
      <SettingToggle label="Checkpoints" description="Save your project's files at the start and end of every agent turn, so you can undo a turn from the History tab. Works in any folder, with any agent. Needs Git installed." checked={settings.checkpoints_enabled ?? true} onChange={(value) => setSettings({ ...settings, checkpoints_enabled: value })} implemented />
      <SettingBlock label="Keep last N checkpoints" implemented description="Per agent. Older checkpoints are deleted.">
        <FieldInput type="number" min={5} max={500} step={5} value={settings.checkpoints_keep ?? 50} disabled={settings.checkpoints_enabled === false} onChange={(e) => setSettings({ ...settings, checkpoints_keep: clampCheckpointsKeep(Number(e.target.value)) })} className="font-mono" />
      </SettingBlock>
      <SettingBlock label="Away digest after (minutes)" implemented description="After this long without keyboard or mouse, Puppet Master gathers what your agents did and shows it when you come back. 0 turns it off.">
        <FieldInput type="number" min={0} max={1440} step={1} value={settings.away_minutes ?? 5} onChange={(e) => setSettings({ ...settings, away_minutes: clampAwayMinutes(Number(e.target.value)) })} className="font-mono" />
      </SettingBlock>
      <SettingToggle label="Auto-save workspace state" description="Persist panes, scrollback, layout, and summaries after important events." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Semantic repository index" description="Build a local index for smarter cross-pane agent context." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Auto session summaries" description="Generate a concise summary when a session is closed or restored." checked onChange={() => undefined} implemented={false} />
      <SettingToggle label="Reduce motion" description="Disable large transitions and decorative animations." checked={false} onChange={() => undefined} implemented={false} />
      <SettingBlock label="App version & updates" implemented description="Checks GitHub releases when the app opens. You can also check manually here.">
        <div className="space-y-3 text-sm">
          <p className="text-pm-muted">
            Installed version: <span className="font-mono text-pm-text">v{installInfo?.version ?? '…'}</span>
            {installInfo && !installInfo.isPackaged ? ' (dev build)' : ''}
          </p>
          {updateCheck?.updateAvailable && updateCheck.latestVersion ? (
            <p className="text-pm-ok">
              Update available: v{updateCheck.latestVersion}. Download the installer from GitHub releases.
            </p>
          ) : updateCheck?.error ? (
            <p className="text-pm-muted">Could not check for updates: {updateCheck.error}</p>
          ) : updateCheck?.latestVersion ? (
            <p className="text-pm-muted">You are on the latest release (v{updateCheck.latestVersion}).</p>
          ) : (
            <p className="text-pm-muted">Update check runs automatically on launch.</p>
          )}
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => onCheckForUpdates?.()}
              disabled={updateChecking}
              className="rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40 disabled:opacity-50"
            >
              {updateChecking ? 'Checking…' : 'Check for updates'}
            </button>
            {updateCheck?.updateAvailable && (
              <button
                type="button"
                onClick={() => onOpenRelease?.()}
                className="rounded-lg border border-pm-accent/50 bg-pm-accent/10 px-3 py-2 text-sm font-medium text-pm-accent hover:bg-pm-accent/20"
              >
                Download & install
              </button>
            )}
          </div>
        </div>
      </SettingBlock>
    </SettingsSection>
  );
}
