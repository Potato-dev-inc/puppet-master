// Development build: every Labs switch reads as on. The app decides (Rust `settings_store::dev_labs_override_active`:
// a debug build, unless PM_DEV_LABS=0); this file only asks once and copies the answer into the settings the
// window reads. It is applied at read time: nothing here is ever saved.
import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';

/** The Labs switches a development build turns on. Mirrors Rust `settings_store::DEV_LABS_KEYS`. */
export const DEV_LABS_KEYS = [
  'missions_enabled',
  'night_shift_enabled',
  'potato_bot_enabled',
  'potato_sandbox_enabled',
  'acp_chat_enabled',
  'desk_enabled',
  'model_bridge_enabled',
] as const;

/** The one line the Labs page shows while the override is on. */
export const DEV_LABS_NOTE = 'Development build: every Labs feature is on.';

let active = false;
let probe: Promise<boolean> | null = null;
const listeners = new Set<(on: boolean) => void>();

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function setActive(on: boolean): void {
  if (on === active) return;
  active = on;
  for (const listener of listeners) listener(on);
}

/** Asks the app once whether the override is on. Outside the desktop app, or if the answer fails, it is off. */
export function probeDevLabs(): Promise<boolean> {
  if (!probe) {
    probe = inTauri()
      ? invoke<unknown>('dev_labs_override_active').then(
          (value) => {
            setActive(value === true);
            return active;
          },
          () => false,
        )
      : Promise.resolve(false);
  }
  return probe;
}

/** Whether the override is on, as last learned (false until the first probe answers). */
export function devLabsOverrideActive(): boolean {
  return active;
}

/** The settings with every Labs switch on while the override is on. The same object when it is off. */
export function withDevLabs<T extends object>(settings: T): T {
  if (!active) return settings;
  return { ...settings, ...Object.fromEntries(DEV_LABS_KEYS.map((key) => [key, true])) };
}

/** Whether the override is on, kept current. Starts the probe the first time it is used. */
export function useDevLabsOverride(): boolean {
  const [on, setOn] = useState(active);
  useEffect(() => {
    let live = true;
    const listener = (value: boolean) => {
      if (live) setOn(value);
    };
    listeners.add(listener);
    void probeDevLabs().then(listener);
    return () => {
      live = false;
      listeners.delete(listener);
    };
  }, []);
  return on;
}

/** Tests only: turns the override on or off as if the app had answered. */
export function setDevLabsOverrideForTests(on: boolean): void {
  setActive(on);
  probe = Promise.resolve(on);
}
