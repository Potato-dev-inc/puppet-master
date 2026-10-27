// "Start Puppet Master when I sign in": reads the OS switch once and writes it through the Tauri commands.
// `ready` is false outside the desktop shell (or when the read fails), so callers can hide the control.
import { useCallback, useEffect, useState } from 'react';
import { tauri } from '../lib/tauri';

export interface StartAtSignIn {
  enabled: boolean;
  ready: boolean;
  error: string | null;
  /** Resolves true when the OS now shows the requested state. */
  change: (value: boolean) => Promise<boolean>;
}

export function useStartAtSignIn(): StartAtSignIn {
  const [enabled, setEnabled] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    // Promise.resolve().then keeps a missing or throwing command from escaping the effect.
    Promise.resolve()
      .then(() => tauri.autostartGet())
      .then((value) => {
        if (!live) return;
        setEnabled(value);
        setReady(true);
      })
      .catch(() => {
        if (live) setReady(false);
      });
    return () => {
      live = false;
    };
  }, []);

  const change = useCallback(async (value: boolean): Promise<boolean> => {
    setError(null);
    try {
      const now = await tauri.autostartSet(value);
      // Show what the OS reports, not what was asked for.
      setEnabled(now);
      return now === value;
    } catch (e) {
      setError(`Could not change it: ${e instanceof Error ? e.message : String(e)}`);
      return false;
    }
  }, []);

  return { enabled, ready, error, change };
}
