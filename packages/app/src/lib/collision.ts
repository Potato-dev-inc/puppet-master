// Collision Alert: the desktop view of two agents that changed the same work (SPEC-n33 section 3.8).
// Backend: `src-tauri/src/collision/` and `commands/collision.rs`. The list is re-read whenever
// the `collision://changed` event fires. Nothing here can pause, stop or steer an agent.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** "clash" is the loudest; "same_file" is quiet; "shared_folder" means two agents worked in one folder. */
export type CollisionLevel = 'clash' | 'same_file' | 'shared_folder';
/** "copies" = two separate copies of one repo; "same_folder" = one folder. */
export type CollisionRelation = 'copies' | 'same_folder';

/** One of the two agent panes in an alert. `label` is the pane heading; no folder, no process text. */
export interface CollisionSide {
  paneId: string;
  label: string;
  working: boolean;
}

/** One file line of an alert. `path` is repo-relative and already cut. */
export interface CollisionFile {
  path: string;
  text: string;
}

/** What the desktop windows get for one alert (camelCase, from `collision/view.rs`). */
export interface CollisionView {
  id: string;
  level: CollisionLevel;
  relation: CollisionRelation;
  title: string;
  detail: string;
  advice: string | null;
  place: string;
  footer: string;
  sides: [CollisionSide, CollisionSide];
  files: CollisionFile[];
  /** Files not listed in `files`. */
  more: number;
  firstMs: number;
  lastMs: number;
}

/** Settings > Agents switch, saved to `collision.json`. */
export interface CollisionSettings {
  enabled: boolean;
}

export const COLLISION_CHANGED_EVENT = 'collision://changed';

/** The alerts that get a banner in a chat and a card in the Inbox. `same_file` stays quiet. */
export function isLoud(view: Pick<CollisionView, 'level'>): boolean {
  return view.level === 'clash' || view.level === 'shared_folder';
}

/** The side that is not `paneId`, or `undefined` when `paneId` is not one of the two panes. */
export function otherSide(view: CollisionView, paneId: string): CollisionSide | undefined {
  const [a, b] = view.sides;
  if (a.paneId === paneId) return b;
  if (b.paneId === paneId) return a;
  return undefined;
}

export interface CollisionApi {
  list: () => Promise<CollisionView[]>;
  ignore: (id: string) => Promise<void>;
  settingsGet: () => Promise<CollisionSettings>;
  settingsSet: (settings: CollisionSettings) => Promise<CollisionSettings>;
  /** Resolves to the unlisten function. Outside the desktop shell it does nothing. */
  onChanged: (cb: () => void) => Promise<() => void>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

/** The last snapshot of visible alerts, newest first. */
export function collisionList(): Promise<CollisionView[]> {
  return call<CollisionView[]>('collision_list');
}

/** Hides one alert until a new file clashes for that pair. The id must look like `ca-` + 12 hex. */
export function collisionIgnore(id: string): Promise<void> {
  return call<void>('collision_ignore', { id });
}

export function collisionSettingsGet(): Promise<CollisionSettings> {
  return call<CollisionSettings>('collision_settings_get');
}

export function collisionSettingsSet(settings: CollisionSettings): Promise<CollisionSettings> {
  return call<CollisionSettings>('collision_settings_set', { settings });
}

/** Calls `cb` each time Rust says the alerts changed. Re-read the list in `cb`. */
export async function onCollisionChanged(cb: () => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return listen(COLLISION_CHANGED_EVENT, () => cb());
}

export const tauriCollisionApi: CollisionApi = {
  list: collisionList,
  ignore: collisionIgnore,
  settingsGet: collisionSettingsGet,
  settingsSet: collisionSettingsSet,
  onChanged: onCollisionChanged,
};
