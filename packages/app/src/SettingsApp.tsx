// Standalone Settings window (label "settings"): the existing Settings UI rendered full-window.
// Closing it closes only this window; the Rust side hides it and the app keeps running in the tray.
import { useMemo } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SettingsPanel } from './components/SettingsPanel';
import { useProjectPath } from './hooks/useProjectPath';
import { settingsEntryTab } from './lib/settings-entry';

export default function SettingsApp() {
  const initialTab = useMemo(() => settingsEntryTab(window.location.search), []);
  const { projectPath, setProjectPath } = useProjectPath();
  return (
    <SettingsPanel
      open
      initialTab={initialTab}
      projectPath={projectPath}
      onProjectPathChange={setProjectPath}
      onClose={() => {
        void getCurrentWindow().close();
      }}
    />
  );
}
