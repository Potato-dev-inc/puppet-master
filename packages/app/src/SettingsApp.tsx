// Standalone Settings window (label "settings"): the Settings panel rendered full-window.
// Closing it closes only this window; the Rust side hides it and the app keeps running in the tray.
// There is no main window, so the update check (and its banner) lives here.
import { useMemo } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SettingsPanel } from './components/SettingsPanel';
import { UpdateAvailableBanner } from './components/UpdateAvailableBanner';
import { useAppUpdateCheck } from './hooks/useAppUpdateCheck';
import { useProjectPath } from './hooks/useProjectPath';
import { settingsEntryTab } from './lib/settings-entry';

export default function SettingsApp() {
  const initialTab = useMemo(() => settingsEntryTab(window.location.search), []);
  const { projectPath, setProjectPath } = useProjectPath();
  const appUpdate = useAppUpdateCheck(true);
  return (
    <>
      <SettingsPanel
        open
        initialTab={initialTab}
        projectPath={projectPath}
        onProjectPathChange={setProjectPath}
        onCheckForUpdates={() => void appUpdate.refresh()}
        updateCheck={appUpdate.result}
        updateChecking={appUpdate.checking}
        onOpenRelease={() => void appUpdate.openRelease()}
        onClose={() => {
          void getCurrentWindow().close();
        }}
      />
      <UpdateAvailableBanner update={appUpdate} />
    </>
  );
}
