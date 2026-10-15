import { useCallback, useEffect, useState } from 'react';
import { checkForAppUpdate, type UpdateCheckResult } from '../lib/app-update';
import { isTauri } from '../lib/cockpit-api';
import { tauri } from '../lib/tauri';

/** A version worth comparing: outside the desktop app the install info falls back to 0.0.0. */
const knownVersion = (version: string | null | undefined): version is string => !!version && version.trim() !== '' && version.trim() !== '0.0.0';

export interface AppUpdateState {
  checking: boolean;
  result: UpdateCheckResult | null;
  dismissed: boolean;
  dismiss: () => void;
  refresh: () => Promise<void>;
  openRelease: () => Promise<void>;
}

export function useAppUpdateCheck(enabled: boolean): AppUpdateState {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateCheckResult | null>(null);
  const [dismissed, setDismissed] = useState(false);

  const refresh = useCallback(async () => {
    // The desktop app's own update, so only inside the desktop app (never the phone page in a browser),
    // and only with a version it knows: "you have v0.0.0" is not a reason to show an update.
    if (!isTauri()) return;
    setChecking(true);
    try {
      const info = await tauri.getAppInstallInfo();
      if (!knownVersion(info.version)) {
        setResult(null);
        return;
      }
      const next = await checkForAppUpdate(info.version);
      setResult(next);
      if (!next.updateAvailable) {
        setDismissed(false);
      }
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    void refresh();
  }, [enabled, refresh]);

  const openRelease = useCallback(async () => {
    const url = result?.releaseUrl;
    if (!url) return;
    await tauri.openExternalUrl(url);
  }, [result?.releaseUrl]);

  return {
    checking,
    result,
    dismissed,
    dismiss: () => setDismissed(true),
    refresh,
    openRelease,
  };
}
