// Settings > Advanced: in-app uninstall, settings export, power-user placeholders.
import { useEffect, useState } from 'react';
import { ask } from '@tauri-apps/plugin-dialog';
import { redactSettings } from '../../../lib/secrets';
import { tauri, type AppInstallInfo } from '../../../lib/tauri';
import { FieldInput, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import type { SettingsTabContext } from './tab-context';

export function AdvancedTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings } = ctx;
  const [installInfo, setInstallInfo] = useState<AppInstallInfo | null>(null);
  const [uninstallError, setUninstallError] = useState<string | null>(null);
  const [uninstalling, setUninstalling] = useState(false);
  useEffect(() => { void tauri.getAppInstallInfo().then(setInstallInfo); }, []);
  const exportSettings = () => {
    const redacted = redactSettings(settings);
    const blob = new Blob([JSON.stringify(redacted, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = 'puppet-master-settings.json';
    anchor.click();
    URL.revokeObjectURL(url);
  };
  const handleUninstall = async () => {
    setUninstallError(null);
    const confirmed = await ask(
      'Puppet Master will close and be removed from this computer. Your app data folder may remain until you delete it manually. Continue?',
      { title: 'Uninstall Puppet Master?', kind: 'warning', okLabel: 'Uninstall', cancelLabel: 'Cancel' },
    );
    if (!confirmed) return;
    setUninstalling(true);
    try {
      await tauri.launchUninstall();
    } catch (error) {
      setUninstallError(error instanceof Error ? error.message : String(error));
      setUninstalling(false);
    }
  };
  return (
    <SettingsSection title="Advanced" description="Power-user options. Change carefully.">
      <SettingBlock label="Uninstall Puppet Master" implemented description={installInfo?.uninstallInstructions ?? 'Loading uninstall instructions…'}>
        <div className="space-y-3">
          {installInfo?.dataDir && (
            <p className="text-xs leading-6 text-pm-muted">
              App data: <span className="font-mono text-pm-text">{installInfo.dataDir}</span>
            </p>
          )}
          {uninstallError && <p className="text-sm text-red-400">{uninstallError}</p>}
          <button
            type="button"
            onClick={() => void handleUninstall()}
            disabled={uninstalling || !installInfo?.uninstallAvailable}
            className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm font-semibold text-red-400 transition hover:bg-red-500/20 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {uninstalling ? 'Uninstalling…' : 'Uninstall Puppet Master'}
          </button>
          {installInfo && !installInfo.uninstallAvailable && (
            <p className="text-xs text-pm-muted">
              In-app uninstall is only available from an installed NSIS/dmg build. For dev builds, stop the app and delete the project folder.
            </p>
          )}
        </div>
      </SettingBlock>
      <SettingBlock label="Scrollback limit" implemented={false}><FieldInput value="10000" disabled className="font-mono" /></SettingBlock>
      <SettingBlock label="Pane spawn timeout" implemented={false}><FieldInput value="30000" disabled className="font-mono" /></SettingBlock>
      <SettingToggle label="Telemetry" description="Send anonymous product diagnostics." checked={false} implemented={false} onChange={() => undefined} />
      <button type="button" onClick={exportSettings} className="rounded-lg border border-pm-border px-3 py-2 text-sm hover:bg-pm-border/40">Export settings JSON</button>
    </SettingsSection>
  );
}
