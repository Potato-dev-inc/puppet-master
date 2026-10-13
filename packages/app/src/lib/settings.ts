import { invoke } from '@tauri-apps/api/core';
import { appConfigDir, join } from '@tauri-apps/api/path';
import { LazyStore } from '@tauri-apps/plugin-store';
import {
  DEFAULT_LLM_MODELS,
  modelKey,
  type LlmModel,
  type LlmProvider,
  type Settings,
} from '@puppet-master/shared';
import { clampAwayMinutes, clampCheckpointsKeep, clampMobileInputDelayMs, toPublicSettings } from './bridge-settings';
import {
  parseDevServerPort,
  publicOriginFromBridgeUrl,
  readStoredPublicBridgeUrl,
} from './public-bridge-url';
import { tauri } from './tauri';

/** Canonical on-disk settings file (app config dir). */
export const SETTINGS_STORE_FILENAME = 'settings.json';
const LEGACY_STORE_FILENAME = 'puppet-master.settings.json';
const SETTINGS_KEY = 'settings';
const LEGACY_SIDEBAR_WIDTH_KEY = 'pm-sidebar-width';

export const MIN_SIDEBAR_WIDTH = 300;
export const MAX_SIDEBAR_WIDTH = 800;
export const SIDEBAR_WIDTH_PRESETS = [300, 360, 400, 480, 640] as const;
export type SidebarWidthPreset = (typeof SIDEBAR_WIDTH_PRESETS)[number];

export const SIDEBAR_WIDTH_PRESET_LABELS: Record<SidebarWidthPreset, string> = {
  300: 'Compact',
  360: 'Comfortable',
  400: 'Balanced',
  480: 'Wide',
  640: 'Extra wide',
};

export function isSidebarWidthPreset(width: number): width is SidebarWidthPreset {
  return (SIDEBAR_WIDTH_PRESETS as readonly number[]).includes(width);
}

let cached: LazyStore | null = null;
let migrationDone = false;

export function clampSidebarWidth(width: number): number {
  if (!Number.isFinite(width)) return 360;
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, Math.round(width)));
}

export type AppTheme = Settings['theme'];

export function applyTheme(theme: AppTheme): void {
  if (typeof document === 'undefined') return;
  document.documentElement.dataset.theme = theme;
}

function store(): LazyStore {
  if (!cached) {
    cached = new LazyStore(SETTINGS_STORE_FILENAME, { defaults: {}, autoSave: true });
  }
  return cached;
}

/** Drop in-memory cache after the bridge writes settings on disk. */
export function invalidateSettingsCache(): void {
  cached = null;
  migrationDone = false;
}

const DEFAULT_SETTINGS: Settings = {
  default_provider: 'anthropic',
  default_model: 'claude-sonnet-4-6',
  custom_models: [],
  orchestrator_backend: 'api',
  mobile_input_delay_ms: 60,
  mobile_input_visible: true,
  mobile_keyboard_resizes_agent: false,
  dev_server_port: 1420,
  sidebar_width: 360,
  theme: 'dark',
  ui_mode: 'orchestration',
  developer_use_rust_mcp: false,
  confirm_close_running_pane: true,
  checkpoints_enabled: true,
  checkpoints_keep: 50,
  away_minutes: 5,
};

function readLegacySidebarWidth(): number | undefined {
  if (typeof localStorage === 'undefined') return undefined;
  const stored = localStorage.getItem(LEGACY_SIDEBAR_WIDTH_KEY);
  if (!stored) return undefined;
  const parsed = Number.parseInt(stored, 10);
  return Number.isFinite(parsed) ? clampSidebarWidth(parsed) : undefined;
}

function mergeLoadedSettings(raw: Settings | null | undefined): Settings {
  const merged = { ...DEFAULT_SETTINGS, ...(raw ?? {}) };
  const legacyPublic = readStoredPublicBridgeUrl();
  const legacySidebar = readLegacySidebarWidth();
  return {
    ...merged,
    mobile_input_delay_ms: clampMobileInputDelayMs(merged.mobile_input_delay_ms),
    mobile_input_visible: merged.mobile_input_visible ?? true,
    mobile_keyboard_resizes_agent: merged.mobile_keyboard_resizes_agent ?? false,
    dev_server_port: parseDevServerPort(merged.dev_server_port),
    sidebar_width: clampSidebarWidth(merged.sidebar_width ?? legacySidebar ?? 360),
    theme: merged.theme ?? 'dark',
    developer_use_rust_mcp: merged.developer_use_rust_mcp ?? false,
    checkpoints_enabled: merged.checkpoints_enabled ?? true,
    checkpoints_keep: clampCheckpointsKeep(merged.checkpoints_keep),
    away_minutes: clampAwayMinutes(merged.away_minutes),
    public_pwa_url:
      merged.public_pwa_url?.trim() ||
      (legacyPublic ? publicOriginFromBridgeUrl(legacyPublic) : undefined),
  };
}

async function migrateLegacySettingsFile(): Promise<void> {
  if (migrationDone) return;
  migrationDone = true;

  try {
    const modern = store();
    const existing = await modern.get<Settings>(SETTINGS_KEY);
    if (existing) return;

    const legacy = new LazyStore(LEGACY_STORE_FILENAME, { defaults: {}, autoSave: false });
    const fromLegacy = await legacy.get<Settings>(SETTINGS_KEY);
    if (!fromLegacy) return;

    await modern.set(SETTINGS_KEY, fromLegacy);
    await modern.save();
  } catch {
    /* migration is best-effort */
  }
}

/** Absolute path to the canonical settings.json in the app config directory. */
export async function resolveSettingsFilePath(): Promise<string> {
  try {
    return join(await appConfigDir(), SETTINGS_STORE_FILENAME);
  } catch {
    return SETTINGS_STORE_FILENAME;
  }
}

export async function loadSettings(): Promise<Settings> {
  await migrateLegacySettingsFile();
  try {
    const raw = await store().get<Settings>(SETTINGS_KEY);
    const merged = mergeLoadedSettings(raw);
    applyTheme(merged.theme ?? 'dark');
    return merged;
  } catch {
    const merged = mergeLoadedSettings(undefined);
    applyTheme(merged.theme ?? 'dark');
    return merged;
  }
}

/** A partial settings object. `null` (or `undefined`) removes a key. */
export type SettingsPatch = { [K in keyof Settings]?: Settings[K] | null };

/** RFC 7386 JSON merge patch: objects merge, `null` removes a key, everything else replaces. */
export function mergePatch<T extends Record<string, unknown>>(target: T, patch: Record<string, unknown>): T {
  const out: Record<string, unknown> = { ...target };
  for (const [key, value] of Object.entries(patch)) {
    if (value === null || value === undefined) delete out[key];
    else if (typeof value === 'object' && !Array.isArray(value) && typeof out[key] === 'object' && out[key] !== null && !Array.isArray(out[key])) {
      out[key] = mergePatch(out[key] as Record<string, unknown>, value as Record<string, unknown>);
    } else out[key] = value;
  }
  return out as T;
}

function normalizePatch(patch: SettingsPatch): Record<string, unknown> {
  const wire: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(patch)) wire[key] = value === undefined ? null : value;
  if (typeof wire.sidebar_width === 'number') wire.sidebar_width = clampSidebarWidth(wire.sidebar_width);
  if (typeof wire.mobile_input_delay_ms === 'number') wire.mobile_input_delay_ms = clampMobileInputDelayMs(wire.mobile_input_delay_ms);
  if ('dev_server_port' in wire && wire.dev_server_port !== null) wire.dev_server_port = parseDevServerPort(wire.dev_server_port as number);
  return wire;
}

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Save only the keys in `patch`. Rust merges them into the file under a lock, so a window that
 * holds an older copy of the settings can never write old values back (the old read-modify-write of
 * the whole object did). `ui_mode` is ignored here: it belongs to `set_ui_mode`. Returns the
 * settings as saved. Outside Tauri (a plain browser) it falls back to the store.
 */
export async function patchSettings(patch: SettingsPatch): Promise<Settings> {
  const wire = normalizePatch(patch);
  let saved: Settings;
  if (inTauri()) {
    saved = mergeLoadedSettings(await invoke<Settings>('settings_patch', { patch: wire }));
  } else {
    const current = (await store().get<Settings>(SETTINGS_KEY)) ?? ({} as Settings);
    const rest = { ...wire };
    delete rest.ui_mode;
    const next = mergePatch(current as unknown as Record<string, unknown>, rest) as unknown as Settings;
    await store().set(SETTINGS_KEY, next);
    await store().save();
    saved = mergeLoadedSettings(next);
  }
  if (typeof wire.theme === 'string') applyTheme(saved.theme ?? 'dark');
  return saved;
}

/**
 * Compatibility shim for callers that still hold a whole settings object: only the keys that differ
 * from what is on disk right now are written, as a patch. New code calls `patchSettings` directly.
 */
export async function saveSettings(s: Settings): Promise<void> {
  const fresh = await loadSettings();
  const patch: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(s)) {
    if (key === 'ui_mode') continue;
    if (JSON.stringify(value) !== JSON.stringify((fresh as unknown as Record<string, unknown>)[key])) patch[key] = value;
  }
  if (Object.keys(patch).length > 0) await patchSettings(patch as SettingsPatch);
}

export async function syncPublicSettingsToBridge(): Promise<void> {
  try {
    const s = await loadSettings();
    await tauri.syncPublicSettings(JSON.stringify(toPublicSettings(s)));
  } catch {
    /* bridge sync is best-effort */
  }
}

/** Preset + user custom models, deduped by provider+model_id (custom wins on label). */
export function listModels(settings?: Settings): LlmModel[] {
  const custom = settings?.custom_models ?? [];
  const byKey = new Map<string, LlmModel>();
  for (const m of DEFAULT_LLM_MODELS) {
    byKey.set(modelKey(m), m);
  }
  for (const m of custom) {
    byKey.set(modelKey(m), m);
  }
  return [...byKey.values()];
}

export function findModel(settings: Settings, provider: LlmProvider, modelId: string): LlmModel | undefined {
  return listModels(settings).find((m) => m.provider === provider && m.model_id === modelId);
}

export function getApiKey(settings: Settings, provider: LlmProvider): string {
  switch (provider) {
    case 'anthropic':
      return settings.anthropic_api_key ?? '';
    case 'openai':
      return settings.openai_api_key ?? '';
    case 'openrouter':
      return settings.openrouter_api_key ?? '';
    default: {
      const _exhaustive: never = provider;
      return _exhaustive;
    }
  }
}

export { modelKey };
