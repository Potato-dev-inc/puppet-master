// Simple mode switch: Settings > General > "Simple mode: fewer buttons and plain words".
// Off unless chosen, so the normal cockpit is unchanged. The last known value is kept in
// localStorage only so the first frame is right; the settings file stays the truth.
import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { loadSettings } from './settings';

const CACHE_KEY = 'pm-simple-mode';

function cached(): boolean {
  try {
    return window.localStorage.getItem(CACHE_KEY) === '1';
  } catch {
    return false;
  }
}

function remember(on: boolean): void {
  try {
    window.localStorage.setItem(CACHE_KEY, on ? '1' : '0');
  } catch {
    /* a private window: the settings file still decides */
  }
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** The value of the switch, kept current when Settings (any window) changes it. */
export function useSimpleMode(): boolean {
  const [on, setOn] = useState<boolean>(cached);
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    const apply = (value: boolean) => {
      if (disposed) return;
      remember(value);
      setOn(value);
    };
    loadSettings()
      .then((s) => apply(s.simple_mode === true))
      .catch(() => undefined);
    if (inTauri()) {
      void listen<{ simple_mode?: unknown } | null>('settings://changed', (event) => {
        const value = event.payload?.simple_mode;
        if (typeof value === 'boolean') apply(value);
      })
        .then((un) => (disposed ? un() : (off = un)))
        .catch(() => undefined);
    }
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
  return on;
}
