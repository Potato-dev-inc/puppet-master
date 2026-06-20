import { appConfigDir, join } from '@tauri-apps/api/path';
import { LazyStore } from '@tauri-apps/plugin-store';
import {
  DEFAULT_LLM_MODELS,
  modelKey,
  type LlmModel,
  type LlmProvider,
  type Settings,
} from '@puppet-master/shared';
import { clampMobileInputDelayMs, toPublicSettings } from './bridge-settings';
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
  mobile_input_delay_ms: 250,
  mobile_input_visible: true,
  dev_server_port: 1420,
  sidebar_width: 360,
  theme: 'dark',
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
    dev_server_port: parseDevServerPort(merged.dev_server_port),
    sidebar_width: clampSidebarWidth(merged.sidebar_width ?? legacySidebar ?? 360),
    theme: merged.theme ?? 'dark',
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

export async function saveSettings(s: Settings): Promise<void> {
  const normalized: Settings = {
    ...s,
    sidebar_width: clampSidebarWidth(s.sidebar_width ?? 360),
    theme: s.theme ?? 'dark',
    mobile_input_delay_ms: clampMobileInputDelayMs(s.mobile_input_delay_ms),
    dev_server_port: parseDevServerPort(s.dev_server_port),
  };
  applyTheme(normalized.theme ?? 'dark');
  await store().set(SETTINGS_KEY, normalized);
  await store().save();
  await syncPublicSettingsToBridge();
  void tauri.pushSettingsEvent(JSON.stringify(toPublicSettings(normalized)));
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
