import type { OrchestratorBackend, Settings } from '@puppet-master/shared';

export function clampMobileInputDelayMs(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return 250;
  const rounded = Math.round(value);
  if (rounded <= 0) return 0;
  return Math.min(1000, Math.max(50, rounded));
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
  | 'developer_use_rust_mcp'
  | 'librarian_indexer_path'
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
    developer_use_rust_mcp: settings.developer_use_rust_mcp ?? false,
    librarian_indexer_path: settings.librarian_indexer_path?.trim() || undefined,
  };
}

export const DEFAULT_PUBLIC_SETTINGS: PublicSettings = {
  orchestrator_backend: 'api',
  default_provider: 'anthropic',
  default_model: 'claude-sonnet-4-6',
  opencode_model_provider: undefined,
  opencode_model_id: undefined,
  mobile_input_delay_ms: 250,
  mobile_input_visible: true,
  developer_use_rust_mcp: false,
  librarian_indexer_path: undefined,
};

export function isOrchestratorBackend(value: string): value is OrchestratorBackend {
  return (
    value === 'api' ||
    value === 'claude_cli' ||
    value === 'codex_cli' ||
    value === 'opencode_cli'
  );
}
