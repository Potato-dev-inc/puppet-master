import type { Settings } from '@puppet-master/shared';

export function clampMobileInputDelayMs(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 250;
  const rounded = Math.round(value);
  if (rounded <= 0) return 0;
  return Math.min(1000, Math.max(50, rounded));
}

export function clampAwayMinutes(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 5;
  return Math.min(1440, Math.max(0, Math.round(value)));
}

export function clampCheckpointsKeep(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 50;
  return Math.min(500, Math.max(5, Math.round(value)));
}

/** Settings fields exposed to the mobile PWA over the HTTP bridge (no API keys). */
export type PublicSettings = Pick<
  Settings,
  | 'orchestrator_backend'
  | 'default_provider'
  | 'default_model'
  | 'opencode_model_provider'
  | 'opencode_model_id'
  | 'mobile_input_delay_ms'
  | 'mobile_input_visible'
  | 'mobile_keyboard_resizes_agent'
  | 'developer_use_rust_mcp'
  | 'librarian_indexer_path'
  | 'checkpoints_enabled'
  | 'checkpoints_keep'
  | 'away_minutes'
>;

export function toPublicSettings(settings: Settings): PublicSettings {
  return {
    orchestrator_backend: settings.orchestrator_backend ?? 'api',
    default_provider: settings.default_provider ?? 'anthropic',
    default_model: settings.default_model ?? 'claude-sonnet-4-6',
    opencode_model_provider: settings.opencode_model_provider?.trim() || undefined,
    opencode_model_id: settings.opencode_model_id?.trim() || undefined,
    mobile_input_delay_ms: clampMobileInputDelayMs(settings.mobile_input_delay_ms),
    mobile_input_visible: settings.mobile_input_visible ?? true,
    mobile_keyboard_resizes_agent: settings.mobile_keyboard_resizes_agent ?? false,
    developer_use_rust_mcp: settings.developer_use_rust_mcp ?? false,
    librarian_indexer_path: settings.librarian_indexer_path?.trim() || undefined,
    checkpoints_enabled: settings.checkpoints_enabled ?? true,
    checkpoints_keep: clampCheckpointsKeep(settings.checkpoints_keep),
    away_minutes: clampAwayMinutes(settings.away_minutes),
  };
}

export const DEFAULT_PUBLIC_SETTINGS: PublicSettings = {
  orchestrator_backend: 'api',
  default_provider: 'anthropic',
  default_model: 'claude-sonnet-4-6',
  opencode_model_provider: undefined,
  opencode_model_id: undefined,
  mobile_input_delay_ms: 60,
  mobile_input_visible: true,
  mobile_keyboard_resizes_agent: false,
  developer_use_rust_mcp: false,
  librarian_indexer_path: undefined,
  checkpoints_enabled: true,
  checkpoints_keep: 50,
  away_minutes: 5,
};
