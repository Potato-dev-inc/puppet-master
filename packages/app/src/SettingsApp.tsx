// Standalone Settings window (label "settings"): the Settings panel rendered full-window. This is Individual
// mode's Settings; in Orchestration mode Settings opens inside the cockpit instead (components/cockpit/SettingsView).
// Closing it closes only this window; the Rust side hides it and the app keeps running in the tray.
// There is no main window, so the update check (and its banner) lives here.
import { useEffect, useMemo, useRef } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SettingsPanel } from './components/SettingsPanel';
import { BringChatsHost } from './components/history/ImportHost';
import type { SettingsPageId } from './components/settings/model';
import { UpdateAvailableBanner } from './components/UpdateAvailableBanner';
import { useAppUpdateCheck } from './hooks/useAppUpdateCheck';
import { useProjectPath } from './hooks/useProjectPath';
import { cockpitApi } from './lib/cockpit-api';
import { settingsEntryTarget } from './lib/settings-target';

export default function SettingsApp() {
  const target = useMemo(() => settingsEntryTarget(window.location.search), []);
  const initialTab = target.page;
  const { projectPath, setProjectPath } = useProjectPath();
  const appUpdate = useAppUpdateCheck(true);
  const page = useRef<SettingsPageId>(initialTab);

  // The mode switched to Orchestration while this window is open: Rust says so (only to a visible window) and
  // Settings moves into the cockpit on the page shown here. `open_settings` in Orchestration mode shows it
  // there and hides this window, so there is never a second Settings surface.
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    void cockpitApi
      .onSettingsToInline(() => {
        void cockpitApi
          .openSettings(page.current)
          // Only after the cockpit took it; if that failed, this window stays so Settings is never lost.
          .then(() => getCurrentWindow().close())
          .catch((err: unknown) => console.error('open_settings failed', err));
      })
      .then((u) => (disposed ? u() : (off = u)));
    return () => {
      disposed = true;
      off?.();
    };
  }, []);

  return (
    <>
      <SettingsPanel
        open
        initialTab={initialTab}
        initialRow={target.row}
        onPageChange={(p) => {
          page.current = p;
        }}
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
      {/* The Bring-chats card in Settings > General asks for its panel here (this window has no cockpit). */}
      <BringChatsHost standalone />
    </>
  );
}
