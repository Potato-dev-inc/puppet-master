import type { Settings } from '@puppet-master/shared';

export function clampMobileInputDelayMs(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 250;
  const rounded = Math.round(value);
  if (rounded <= 0) return 0;
  return Math.min(1000, Math.max(50, rounded));
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
  | 'confirm_close_running_pane'
  | 'librarian_indexer_path'
  | 'checkpoints_enabled'
  | 'checkpoints_keep'
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
    confirm_close_running_pane: settings.confirm_close_running_pane ?? true,
    librarian_indexer_path: settings.librarian_indexer_path?.trim() || undefined,
    checkpoints_enabled: settings.checkpoints_enabled ?? true,
    checkpoints_keep: clampCheckpointsKeep(settings.checkpoints_keep),
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
  confirm_close_running_pane: true,
  checkpoints_enabled: true,
  checkpoints_keep: 50,
};
