import type { LlmProvider, OrchestratorBackend } from '@puppet-master/shared';
import type { BridgeClient } from './bridge';
import { loadSettings, saveSettings } from './settings';

/**
 * The orchestrator "provider" state that must stay in sync between the desktop
 * app and the mobile PWA. It combines the orchestration backend (API, Claude
 * Code, Codex CLI, OpenCode CLI) with the default LLM provider/model used by
 * the direct API backend.
 */
export interface OrchestratorProvider {
  backend: OrchestratorBackend;
  provider: LlmProvider;
  model: string;
}

/**
 * Apply a remote orchestrator-provider change (e.g. initiated by the mobile PWA
 * over the bridge SSE stream) to the desktop settings store.
 *
 * This also re-broadcasts the updated public settings back over the bridge SSE
 * stream via `saveSettings`, so every connected client — including the mobile
 * PWA that sent the change — stays in sync.
 */
export async function applyRemoteOrchestratorProviderPatch(
  patch: Partial<OrchestratorProvider>,
): Promise<OrchestratorProvider> {
  const current = await loadSettings();
  const next = { ...current };

  if (patch.backend) {
    next.orchestrator_backend = patch.backend;
  }
  if (patch.provider) {
    next.default_provider = patch.provider;
  }
  if (patch.model) {
    next.default_model = patch.model;
  }

  await saveSettings(next);

  return {
    backend: next.orchestrator_backend,
    provider: next.default_provider,
    model: next.default_model,
  };
}

/**
 * Convert the public settings patch fields used by the HTTP bridge (or the
 * Tauri `settings://apply` event) into the typed `OrchestratorProvider` shape.
 */
export function publicSettingsToProviderPatch(
  patch: Partial<{
    orchestrator_backend?: string;
    default_provider?: string;
    default_model?: string;
  }>,
): Partial<OrchestratorProvider> {
  const result: Partial<OrchestratorProvider> = {};
  if (patch.orchestrator_backend) {
    result.backend = patch.orchestrator_backend as OrchestratorBackend;
  }
  if (patch.default_provider) {
    result.provider = patch.default_provider as LlmProvider;
  }
  if (patch.default_model) {
    result.model = patch.default_model;
  }
  return result;
}

/**
 * Push an orchestrator-provider change from the mobile PWA to the desktop
 * bridge. The bridge will persist the change, notify the desktop UI, and
 * broadcast the updated settings to all SSE clients so both sides stay in sync.
 */
export async function pushOrchestratorProviderChange(
  bridge: BridgeClient,
  patch: Partial<OrchestratorProvider>,
): Promise<void> {
  const publicPatch: Partial<{
    orchestrator_backend: OrchestratorBackend;
    default_provider: LlmProvider;
    default_model: string;
  }> = {};

  if (patch.backend) {
    publicPatch.orchestrator_backend = patch.backend;
  }
  if (patch.provider) {
    publicPatch.default_provider = patch.provider;
  }
  if (patch.model) {
    publicPatch.default_model = patch.model;
  }

  await bridge.patchSettings(publicPatch);
}
