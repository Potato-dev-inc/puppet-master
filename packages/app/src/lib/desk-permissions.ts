// What this computer still needs switched on (the desktop app's `desk_permissions` command, src-tauri/src/desk/permissions.rs).
// Only a Mac asks (Accessibility); anywhere else `accessibility` is null and nothing is shown. No sample data here.
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface DeskPermissions {
  /** `false`: the person has not turned it on. `null`: this computer does not ask for it. */
  accessibility: boolean | null;
}

export const deskPermissions = () => invoke<DeskPermissions>('desk_permissions');

function isPermissions(v: unknown): v is DeskPermissions {
  if (typeof v !== 'object' || v === null) return false;
  const a = (v as DeskPermissions).accessibility;
  return a === null || typeof a === 'boolean';
}

/** The words for what to switch on, or null when nothing needs doing. Pure, so the page and its tests agree. */
export function accessibilityHelp(p: DeskPermissions | null): { title: string; steps: string } | null {
  if (!p || p.accessibility !== false) return null;
  return {
    title: 'This Mac has not allowed Puppet Master to control the mouse and keyboard yet.',
    steps:
      'Open System Settings, go to Privacy & Security, then Accessibility, and switch on Puppet Master. If it is not in the list, add it with the + button. If you started Puppet Master from a terminal (npm run tauri dev), switch on that terminal app instead and start it again. Then come back here.',
  };
}

export interface PermissionsApi {
  permissions: DeskPermissions | null;
  /** Ask again (the person came back from System Settings). */
  recheck: () => void;
}

/**
 * Asks once when `enabled`, and again whenever the person returns to this window: that is when they have just
 * been to System Settings. A command that fails (an older app, Labs off) leaves it at null: nothing is shown.
 */
export function useDeskPermissions(enabled: boolean): PermissionsApi {
  const [permissions, setPermissions] = useState<DeskPermissions | null>(null);
  const alive = useRef(true);

  const recheck = useCallback(() => {
    Promise.resolve(deskPermissions())
      .then((p) => {
        if (alive.current && isPermissions(p)) setPermissions(p);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    alive.current = true;
    if (!enabled) return () => void (alive.current = false);
    recheck();
    const onShow = () => {
      if (document.visibilityState !== 'hidden') recheck();
    };
    window.addEventListener('focus', onShow);
    document.addEventListener('visibilitychange', onShow);
    return () => {
      alive.current = false;
      window.removeEventListener('focus', onShow);
      document.removeEventListener('visibilitychange', onShow);
    };
  }, [enabled, recheck]);

  return { permissions, recheck };
}
