import { describe, expect, it, vi } from 'vitest';
import {
  applyRemoteOrchestratorProviderPatch,
  publicSettingsToProviderPatch,
  pushOrchestratorProviderChange,
} from './orchestrator-provider-sync';

vi.mock('./settings', () => ({
  loadSettings: vi.fn(async () => ({
    default_provider: 'anthropic',
    default_model: 'claude-sonnet-4-6',
    custom_models: [],
    orchestrator_backend: 'api',
    mobile_input_delay_ms: 250,
    mobile_input_visible: true,
    dev_server_port: 1420,
  })),
  saveSettings: vi.fn(async () => {}),
}));

describe('orchestrator-provider-sync', () => {
  describe('publicSettingsToProviderPatch', () => {
    it('maps raw bridge patch fields to the typed provider shape', () => {
      expect(
        publicSettingsToProviderPatch({
          orchestrator_backend: 'claude_cli',
          default_provider: 'openai',
          default_model: 'gpt-4.1',
        }),
      ).toEqual({
        backend: 'claude_cli',
        provider: 'openai',
        model: 'gpt-4.1',
      });
    });

    it('ignores unrelated fields', () => {
      expect(
        publicSettingsToProviderPatch({
          orchestrator_backend: 'codex_cli',
          mobile_input_delay_ms: 500,
        } as Record<string, unknown>),
      ).toEqual({ backend: 'codex_cli' });
    });
  });

  describe('applyRemoteOrchestratorProviderPatch', () => {
    it('persists a remote backend change and returns the new provider state', async () => {
      const result = await applyRemoteOrchestratorProviderPatch({
        backend: 'opencode_cli',
      });
      expect(result.backend).toBe('opencode_cli');
      const { saveSettings } = await import('./settings');
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          orchestrator_backend: 'opencode_cli',
          default_provider: 'anthropic',
          default_model: 'claude-sonnet-4-6',
        }),
      );
    });
  });

  describe('pushOrchestratorProviderChange', () => {
    it('sends a typed provider patch through the bridge client', async () => {
      const patchSettings = vi.fn().mockResolvedValue(undefined);
      const bridge = { patchSettings } as unknown as Parameters<
        typeof pushOrchestratorProviderChange
      >[0];

      await pushOrchestratorProviderChange(bridge, {
        backend: 'claude_cli',
        provider: 'openai',
        model: 'gpt-4.1',
      });

      expect(patchSettings).toHaveBeenCalledWith({
        orchestrator_backend: 'claude_cli',
        default_provider: 'openai',
        default_model: 'gpt-4.1',
      });
    });
  });
});
