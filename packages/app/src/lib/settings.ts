import { LazyStore } from '@tauri-apps/plugin-store';
import {
  DEFAULT_LLM_MODELS,
  modelKey,
  type LlmModel,
  type LlmProvider,
  type Settings,
} from '@puppet-master/shared';
import { toPublicSettings } from './bridge-settings';
import { tauri } from './tauri';

const STORE_FILE = 'puppet-master.settings.json';
const KEY = 'settings';

let cached: LazyStore | null = null;

function store(): LazyStore {
  if (!cached) {
    cached = new LazyStore(STORE_FILE, { defaults: {}, autoSave: true });
  }
  return cached;
}

/** Drop in-memory cache after the bridge writes settings on disk. */
export function invalidateSettingsCache(): void {
  cached = null;
}

const DEFAULT_SETTINGS: Settings = {
  default_provider: 'anthropic',
  default_model: 'claude-sonnet-4-6',
  custom_models: [],
  orchestrator_backend: 'api',
};

export async function loadSettings(): Promise<Settings> {
  try {
    const raw = await store().get<Settings>(KEY);
    return { ...DEFAULT_SETTINGS, ...(raw ?? {}) };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export async function saveSettings(s: Settings): Promise<void> {
  await store().set(KEY, s);
  await store().save();
  await syncPublicSettingsToBridge();
  void tauri.pushSettingsEvent(JSON.stringify(toPublicSettings(s)));
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
