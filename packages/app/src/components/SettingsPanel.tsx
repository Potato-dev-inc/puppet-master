import { useEffect, useState } from 'react';
import { listModels, loadSettings, saveSettings } from '../lib/settings';
import type { LlmModel, LlmProvider, Settings } from '@puppet-master/shared';

interface Props {
  open: boolean;
  onClose: () => void;
  onSaved?: () => void;
}

const EMPTY_CUSTOM: LlmModel = {
  provider: 'openrouter',
  model_id: '',
  label: '',
};

export function SettingsPanel({ open, onClose, onSaved }: Props) {
  const [settings, setSettings] = useState<Settings>({
    default_provider: 'anthropic',
    default_model: 'claude-sonnet-4-6',
    custom_models: [],
    orchestrator_backend: 'api',
    mobile_input_delay_ms: 5000,
  });
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [draftCustom, setDraftCustom] = useState<LlmModel>(EMPTY_CUSTOM);

  useEffect(() => {
    if (open) {
      void loadSettings().then((s) => setSettings(s));
      setDraftCustom(EMPTY_CUSTOM);
    }
  }, [open]);

  if (!open) return null;

  const models = listModels(settings).filter((m) => m.provider === settings.default_provider);
  const customForProvider = (settings.custom_models ?? []).filter((m) => m.provider === settings.default_provider);

  const addCustomModel = () => {
    const label = draftCustom.label.trim();
    const model_id = draftCustom.model_id.trim();
    if (!label || !model_id) return;
    const entry: LlmModel = { provider: draftCustom.provider, model_id, label };
    const next = [...(settings.custom_models ?? []).filter((m) => !(m.provider === entry.provider && m.model_id === entry.model_id)), entry];
    setSettings({ ...settings, custom_models: next, default_provider: entry.provider, default_model: entry.model_id });
    setDraftCustom({ ...EMPTY_CUSTOM, provider: draftCustom.provider });
  };

  const removeCustomModel = (target: LlmModel) => {
    const next = (settings.custom_models ?? []).filter(
      (m) => !(m.provider === target.provider && m.model_id === target.model_id),
    );
    setSettings({ ...settings, custom_models: next });
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/60 flex items-center justify-center" onClick={onClose}>
      <div
        className="bg-pm-panel border border-pm-border rounded-lg p-4 w-[520px] max-w-[92vw] max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="text-sm font-semibold mb-3">Puppet Master settings</h2>

        <label className="block text-xs text-pm-muted mb-1">Provider</label>
        <select
          value={settings.default_provider}
          onChange={(e) => {
            const provider = e.target.value as Settings['default_provider'];
            const first = listModels(settings).find((m) => m.provider === provider);
            setSettings({
              ...settings,
              default_provider: provider,
              default_model: first?.model_id ?? settings.default_model,
            });
            setDraftCustom((d) => ({ ...d, provider }));
          }}
          className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1 mb-3"
        >
          <option value="anthropic">Anthropic</option>
          <option value="openai">OpenAI</option>
          <option value="openrouter">OpenRouter</option>
        </select>

        <label className="block text-xs text-pm-muted mb-1">Default model</label>
        <select
          value={settings.default_model}
          onChange={(e) => setSettings({ ...settings, default_model: e.target.value })}
          className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1 mb-3"
        >
          {models.map((m) => (
            <option key={`${m.provider}::${m.model_id}`} value={m.model_id}>
              {m.label}
            </option>
          ))}
        </select>

        <div className="border border-pm-border rounded p-3 mb-3">
          <h3 className="text-xs font-semibold mb-2">Custom models</h3>
          <p className="text-[10px] text-pm-muted mb-2">
            Add any model id (e.g. <code className="font-mono">google/gemini-2.5-pro</code> on OpenRouter). They appear in the sidebar picker.
          </p>

          {customForProvider.length > 0 && (
            <ul className="mb-2 space-y-1">
              {customForProvider.map((m) => (
                <li key={`${m.provider}::${m.model_id}`} className="flex items-center gap-2 text-xs font-mono">
                  <span className="flex-1 truncate">{m.label} — {m.model_id}</span>
                  <button
                    type="button"
                    onClick={() => removeCustomModel(m)}
                    className="px-1.5 py-0.5 rounded text-pm-err hover:bg-pm-err/10"
                  >
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}

          <div className="grid grid-cols-2 gap-2 mb-2">
            <div>
              <label className="block text-[10px] text-pm-muted mb-0.5">Provider</label>
              <select
                value={draftCustom.provider}
                onChange={(e) => setDraftCustom({ ...draftCustom, provider: e.target.value as LlmProvider })}
                className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1"
              >
                <option value="anthropic">Anthropic</option>
                <option value="openai">OpenAI</option>
                <option value="openrouter">OpenRouter</option>
              </select>
            </div>
            <div>
              <label className="block text-[10px] text-pm-muted mb-0.5">Display label</label>
              <input
                value={draftCustom.label}
                onChange={(e) => setDraftCustom({ ...draftCustom, label: e.target.value })}
                className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1"
                placeholder="My model"
              />
            </div>
          </div>
          <div className="flex gap-2">
            <input
              value={draftCustom.model_id}
              onChange={(e) => setDraftCustom({ ...draftCustom, model_id: e.target.value })}
              className="flex-1 text-xs bg-pm-bg border border-pm-border rounded px-2 py-1 font-mono"
              placeholder="model-id or vendor/model"
            />
            <button
              type="button"
              onClick={addCustomModel}
              disabled={!draftCustom.label.trim() || !draftCustom.model_id.trim()}
              className="px-2 py-1 text-xs rounded border border-pm-accent bg-pm-accent/20 text-pm-accent hover:bg-pm-accent/30 disabled:opacity-50"
            >
              Add
            </button>
          </div>
        </div>

        <label className="block text-xs text-pm-muted mb-1">Mobile input buffer (ms)</label>
        <input
          type="number"
          min={250}
          max={10000}
          step={50}
          value={settings.mobile_input_delay_ms ?? 5000}
          onChange={(e) => {
            const value = Number(e.target.value);
            setSettings({
              ...settings,
              mobile_input_delay_ms: Number.isFinite(value)
                ? Math.min(10000, Math.max(250, Math.round(value)))
                : 5000,
            });
          }}
          className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1 mb-3 font-mono"
        />

        <label className="block text-xs text-pm-muted mb-1">Anthropic API key</label>
        <input
          type="password"
          value={settings.anthropic_api_key ?? ''}
          onChange={(e) => setSettings({ ...settings, anthropic_api_key: e.target.value })}
          className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1 mb-3 font-mono"
          placeholder="sk-ant-…"
        />

        <label className="block text-xs text-pm-muted mb-1">OpenAI API key</label>
        <input
          type="password"
          value={settings.openai_api_key ?? ''}
          onChange={(e) => setSettings({ ...settings, openai_api_key: e.target.value })}
          className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1 mb-3 font-mono"
          placeholder="sk-…"
        />

        <label className="block text-xs text-pm-muted mb-1">OpenRouter API key</label>
        <input
          type="password"
          value={settings.openrouter_api_key ?? ''}
          onChange={(e) => setSettings({ ...settings, openrouter_api_key: e.target.value })}
          className="w-full text-xs bg-pm-bg border border-pm-border rounded px-2 py-1 mb-3 font-mono"
          placeholder="sk-or-…"
        />

        <div className="flex justify-end gap-2 mt-4">
          {savedAt && <span className="text-xs text-pm-ok self-center mr-auto">Saved</span>}
          <button onClick={onClose} className="px-3 py-1 text-xs rounded border border-pm-border hover:bg-pm-border/40">
            Close
          </button>
          <button
            onClick={async () => {
              await saveSettings(settings);
              setSavedAt(Date.now());
              onSaved?.();
            }}
            className="px-3 py-1 text-xs rounded border border-pm-accent bg-pm-accent/20 text-pm-accent hover:bg-pm-accent/30"
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
