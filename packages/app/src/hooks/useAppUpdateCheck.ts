import { useCallback, useEffect, useState } from 'react';
import { checkForAppUpdate, type UpdateCheckResult } from '../lib/app-update';
import { tauri } from '../lib/tauri';

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
    setChecking(true);
    try {
      const info = await tauri.getAppInstallInfo();
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
