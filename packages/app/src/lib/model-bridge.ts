// Model Bridge (Labs `model_bridge_enabled`, off by default): connect a model service once, then add it to Codex.
// Wire types and calls for the Rust commands in `commands/model_bridge.rs`. A key goes in (`saveKey`) and never comes
// back: no call here returns one. Errors are plain sentences made by the app.
import { invoke } from '@tauri-apps/api/core';

export interface ModelBridgeApplied {
  recipe: string;
  envVar: string;
}

export interface ModelBridgeService {
  id: string;
  displayName: string;
  note: string | null;
  limitsNote: string | null;
  needsReview: boolean;
  /** The blank service: the person types a name and an address. */
  blank: boolean;
  baseUrl: string;
  shapes: string[];
  keySaved: boolean;
  codexDirect: boolean;
  applied: ModelBridgeApplied | null;
  envLine: string;
}

export interface ModelBridgeList {
  enabled: boolean;
  secure: boolean;
  storageLabel: string;
  adapters: ModelBridgeService[];
  problems: string[];
}

export interface ModelBridgePreview {
  title: string;
  configPath: string;
  lines: string[];
  envLine: string;
  service: string;
}

export interface ModelBridgeDone {
  message: string;
  backupPath: string | null;
}

export interface ModelBridgeApi {
  list(): Promise<ModelBridgeList>;
  /** Resolves with the name of the place the key is kept. */
  saveKey(adapterId: string, key: string): Promise<string>;
  remove(adapterId: string): Promise<void>;
  addCustom(name: string, url: string): Promise<string>;
  preview(adapterId: string): Promise<ModelBridgePreview>;
  apply(adapterId: string): Promise<ModelBridgeDone>;
  undo(adapterId: string): Promise<ModelBridgeDone>;
}

export const modelBridgeApi: ModelBridgeApi = {
  list: () => invoke<ModelBridgeList>('model_bridge_list'),
  saveKey: (adapterId, key) => invoke<string>('model_bridge_save_key', { adapterId, key }),
  remove: (adapterId) => invoke<void>('model_bridge_remove', { adapterId }),
  addCustom: (name, url) => invoke<string>('model_bridge_add_custom', { name, url }),
  preview: (adapterId) => invoke<ModelBridgePreview>('model_bridge_preview', { adapterId }),
  apply: (adapterId) => invoke<ModelBridgeDone>('model_bridge_apply', { adapterId }),
  undo: (adapterId) => invoke<ModelBridgeDone>('model_bridge_undo', { adapterId }),
};

/** The error as one plain sentence (the app's own text, without a type prefix). */
export function plainError(e: unknown): string {
  const text = typeof e === 'string' ? e : e instanceof Error ? e.message : '';
  const cleaned = text.replace(/^(Error|INVALID_ARGUMENT):\s*/i, '').trim();
  return cleaned || 'Something went wrong. Nothing was changed.';
}

/** "Key saved in Windows Credential Manager." */
export function keySavedLine(storageLabel: string): string {
  return `Key saved in ${storageLabel}.`;
}
