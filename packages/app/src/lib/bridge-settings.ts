import type { OrchestratorBackend, Settings } from '@puppet-master/shared';

/** Settings fields exposed to the mobile PWA over the HTTP bridge (no API keys). */
export type PublicSettings = Pick<
  Settings,
  'orchestrator_backend' | 'default_provider' | 'default_model'
>;

export function toPublicSettings(settings: Settings): PublicSettings {
  return {
    orchestrator_backend: settings.orchestrator_backend ?? 'api',
    default_provider: settings.default_provider ?? 'anthropic',
    default_model: settings.default_model ?? 'claude-sonnet-4-6',
  };
}

export const DEFAULT_PUBLIC_SETTINGS: PublicSettings = {
  orchestrator_backend: 'api',
  default_provider: 'anthropic',
  default_model: 'claude-sonnet-4-6',
};

export function isOrchestratorBackend(value: string): value is OrchestratorBackend {
  return (
    value === 'api' ||
    value === 'claude_cli' ||
    value === 'codex_cli' ||
    value === 'opencode_cli'
  );
}
