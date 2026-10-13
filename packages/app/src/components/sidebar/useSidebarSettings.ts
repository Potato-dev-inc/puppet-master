// Sidebar-facing view of persisted settings: orchestrator backend, model list/selection and theme,
// kept in sync with the settings store and the "settings changed" desktop event.
import { useCallback, useEffect, useState } from 'react';
import { DEFAULT_LLM_MODELS, type LlmModel, type OrchestratorBackend } from '@puppet-master/shared';
import { findModel, invalidateSettingsCache, listModels, loadSettings, patchSettings } from '../../lib/settings';
import { tauri } from '../../lib/tauri';

export function useSidebarSettings(settingsRevision: number) {
  const [model, setModel] = useState<LlmModel>(DEFAULT_LLM_MODELS[0]);
  const [availableModels, setAvailableModels] = useState<LlmModel[]>(DEFAULT_LLM_MODELS);
  const [backend, setBackend] = useState<OrchestratorBackend>('api');
  const [theme, setTheme] = useState<'dark' | 'light'>('dark');

  const refreshFromSettings = useCallback(async () => {
    const s = await loadSettings();
    const models = listModels(s);
    setAvailableModels(models);
    setBackend(s.orchestrator_backend ?? 'api');
    setTheme(s.theme ?? 'dark');
    const found = findModel(s, s.default_provider, s.default_model) ?? models[0];
    if (found) setModel(found);
  }, []);

  useEffect(() => {
    void refreshFromSettings();
  }, [refreshFromSettings, settingsRevision]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void (async () => {
      unlisten = await tauri.onSettingsChanged((payload) => {
        invalidateSettingsCache();
        if (payload.orchestrator_backend) {
          setBackend(payload.orchestrator_backend as OrchestratorBackend);
        }
        void refreshFromSettings();
      });
      if (cancelled) unlisten?.();
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refreshFromSettings]);

  const changeBackend = useCallback(async (next: OrchestratorBackend) => {
    setBackend(next);
    await patchSettings({ orchestrator_backend: next });
  }, []);

  const changeModel = useCallback(async (next: LlmModel) => {
    setModel(next);
    await patchSettings({ default_provider: next.provider, default_model: next.model_id });
  }, []);

  const toggleTheme = useCallback(async () => {
    const next = theme === 'dark' ? 'light' : 'dark';
    setTheme(next);
    await patchSettings({ theme: next });
  }, [theme]);

  return { model, availableModels, backend, theme, changeBackend, changeModel, toggleTheme };
}
