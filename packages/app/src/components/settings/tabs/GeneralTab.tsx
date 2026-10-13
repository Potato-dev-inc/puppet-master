// Settings > General: settings file location, default project folder, theme, app version and update check.
import { useEffect, useState } from 'react';
import { homeDir } from '@tauri-apps/api/path';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type { Settings } from '@puppet-master/shared';
import { resolveSettingsFilePath } from '../../../lib/settings';
import { tauri, type AppInstallInfo } from '../../../lib/tauri';
import { FieldInput, FieldSelect, InfoCard, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import { SharedMemoryToggle } from './SharedMemoryToggle';
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
      <SharedMemoryToggle />
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
