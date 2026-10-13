// Provider API keys live in the OS credential store (Rust `secrets.rs`). The UI only ever sees
// "is it set" plus the last 4 characters. The one exception is `resolveApiKey`, used by the
// sidebar's API orchestrator whose LLM loop runs in this webview (documented in docs/features/trust-fixes.md).
import { invoke } from '@tauri-apps/api/core';
import type { LlmProvider, Settings } from '@puppet-master/shared';

export type SecretName = 'anthropic_api_key' | 'openai_api_key' | 'openrouter_api_key';

export interface SecretMask {
  set: boolean;
  last4: string | null;
}

export interface SecretStorageInfo {
  secure: boolean;
  label: string;
}

export interface SecretsApi {
  storageInfo(): Promise<SecretStorageInfo>;
  getMasked(name: SecretName): Promise<SecretMask>;
  set(name: SecretName, value: string): Promise<void>;
  remove(name: SecretName): Promise<void>;
}

export const secretsApi: SecretsApi = {
  storageInfo: () => invoke<SecretStorageInfo>('secret_storage_info'),
  getMasked: (name) => invoke<SecretMask>('secret_get_masked', { name }),
  set: (name, value) => invoke<void>('secret_set', { name, value }),
  remove: (name) => invoke<void>('secret_delete', { name }),
};

const PROVIDER_SECRET: Record<LlmProvider, SecretName> = {
  anthropic: 'anthropic_api_key',
  openai: 'openai_api_key',
  openrouter: 'openrouter_api_key',
};

/** Mask a key held in the plain settings file (fallback when no credential store exists). */
export function maskLocal(value: string | undefined): SecretMask {
  const v = value ?? '';
  if (!v) return { set: false, last4: null };
  return { set: true, last4: v.length >= 12 ? v.slice(-4) : null };
}

/** Plain, true-in-every-state line describing a key. */
export function describeKeyState(mask: SecretMask, info: SecretStorageInfo): string {
  if (!mask.set) {
    return info.secure ? `Not set. It will be stored in ${info.label}.` : 'Not set.';
  }
  const dots = mask.last4 ? `•••• ${mask.last4}` : '••••';
  return info.secure
    ? `${dots} · Stored in ${info.label}`
    : `${dots} · Saved in a plain settings file (not secure)`;
}

export const NO_SECURE_STORAGE_WARNING =
  'Secure storage is not available on this computer. Keys you enter are saved in a plain file in the app folder.';

/** The key for an LLM call: credential store first, then the settings file (fallback mode). */
export async function resolveApiKey(settings: Settings, provider: LlmProvider): Promise<string> {
  const name = PROVIDER_SECRET[provider];
  try {
    const raw = await invoke<string | null>('secret_get_for_llm', { name });
    if (raw) return raw;
  } catch {
    /* no Tauri, or no store: fall back to the file value */
  }
  return (settings[name] as string | undefined) ?? '';
}

const SECRET_KEY_PATTERN = /(api_?key|token|secret|password)/i;

/** Settings safe to export: any secret-looking field is replaced, never copied. */
export function redactSettings<T extends object>(settings: T): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(settings)) {
    if (SECRET_KEY_PATTERN.test(k)) {
      if (typeof v === 'string' && v.length > 0) out[k] = '***';
      continue;
    }
    out[k] = v;
  }
  return out;
}

/** Drop provider keys from a settings object that is about to be written to settings.json. */
export function withoutSecrets(s: Settings): Settings {
  const copy = { ...s };
  delete copy.anthropic_api_key;
  delete copy.openai_api_key;
  delete copy.openrouter_api_key;
  return copy;
}
