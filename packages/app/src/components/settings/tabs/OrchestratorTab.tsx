// Settings > Orchestrator: backend, librarian indexer, OpenCode default model, API default provider/model, custom models.
import type { LlmModel, LlmProvider } from '@puppet-master/shared';
import { listModels } from '../../../lib/settings';
import { FieldInput, FieldLabel, FieldSelect, InfoCard, SettingBlock, SettingToggle, SettingsSection } from '../settings-ui';
import { OrchestratorBackendSelect } from './OrchestratorBackendSelect';
import { EMPTY_CUSTOM, type SettingsTabContext } from './tab-context';

export function OrchestratorTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, draftCustom, setDraftCustom } = ctx;
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
    const next = (settings.custom_models ?? []).filter((m) => !(m.provider === target.provider && m.model_id === target.model_id));
    setSettings({ ...settings, custom_models: next });
  };
  return (
    <SettingsSection title="Orchestrator" description="Configure the Puppet Master sidebar brain and delegation logic.">
      <SettingBlock label="Backend" implemented>
        <OrchestratorBackendSelect settings={settings} setSettings={setSettings} />
      </SettingBlock>
      <SettingToggle label="Cost-aware model router" description="Route tasks to cheaper or stronger models depending on complexity." checked onChange={() => undefined} implemented={false} />
      <SettingBlock
        label="Librarian indexer"
        implemented
        description="Default: OpenCode librarian (read_librarian_prompt → opencode_native). Use opencode or leave blank. Or scripts/project-indexer.py for static-only. Edit scripts/librarian-prompt.md to customize the OpenCode task."
      >
        <FieldInput
          value={settings.librarian_indexer_path ?? 'opencode'}
          onChange={(e) => setSettings({ ...settings, librarian_indexer_path: e.target.value || undefined })}
          className="font-mono"
          placeholder="opencode"
          autoComplete="off"
        />
      </SettingBlock>
      <SettingBlock
        label="OpenCode default model"
        implemented
        description="Used for opencode_native API prompts when write_terminal_input omits model_provider/model_id."
      >
        <div className="space-y-3">
          <div>
            <FieldLabel>Provider ID</FieldLabel>
            <FieldInput
              value={settings.opencode_model_provider ?? ''}
              onChange={(e) => setSettings({ ...settings, opencode_model_provider: e.target.value || undefined })}
              className="font-mono"
              placeholder="anthropic"
              autoComplete="off"
            />
          </div>
          <div>
            <FieldLabel>Model ID</FieldLabel>
            <FieldInput
              value={settings.opencode_model_id ?? ''}
              onChange={(e) => setSettings({ ...settings, opencode_model_id: e.target.value || undefined })}
              className="font-mono"
              placeholder="claude-sonnet-4"
              autoComplete="off"
            />
          </div>
        </div>
      </SettingBlock>
      <SettingBlock label="Default provider & model" implemented description="Used by the API orchestrator backend.">
        <div className="space-y-3">
          <div>
            <FieldLabel>Provider</FieldLabel>
            <FieldSelect value={settings.default_provider} onChange={(e) => {
              const provider = e.target.value as LlmProvider;
              const first = listModels(settings).find((m) => m.provider === provider);
              setSettings({ ...settings, default_provider: provider, default_model: first?.model_id ?? settings.default_model });
              setDraftCustom({ ...draftCustom, provider });
            }}>
              <option value="anthropic">Anthropic</option><option value="openai">OpenAI</option><option value="openrouter">OpenRouter</option>
            </FieldSelect>
          </div>
          <div>
            <FieldLabel>Model</FieldLabel>
            <FieldSelect value={settings.default_model} onChange={(e) => setSettings({ ...settings, default_model: e.target.value })}>
              {models.map((m) => (<option key={`${m.provider}::${m.model_id}`} value={m.model_id}>{m.label}</option>))}
            </FieldSelect>
          </div>
        </div>
      </SettingBlock>
      <SettingBlock label="Custom models" implemented description="Add vendor/model ids for the sidebar picker.">
        {customForProvider.length > 0 && (
          <ul className="mb-3 space-y-1">
            {customForProvider.map((m) => (
              <li key={`${m.provider}::${m.model_id}`} className="flex items-center gap-2 text-xs font-mono">
                <span className="min-w-0 flex-1 truncate">{m.label} — {m.model_id}</span>
                <button type="button" onClick={() => removeCustomModel(m)} className="rounded px-1.5 py-0.5 text-pm-err hover:bg-pm-err/10">Remove</button>
              </li>
            ))}
          </ul>
        )}
        <div className="grid grid-cols-2 gap-2">
          <div><FieldLabel>Provider</FieldLabel><FieldSelect value={draftCustom.provider} onChange={(e) => setDraftCustom({ ...draftCustom, provider: e.target.value as LlmProvider })}><option value="anthropic">Anthropic</option><option value="openai">OpenAI</option><option value="openrouter">OpenRouter</option></FieldSelect></div>
          <div><FieldLabel>Display label</FieldLabel><FieldInput value={draftCustom.label} onChange={(e) => setDraftCustom({ ...draftCustom, label: e.target.value })} placeholder="My model" /></div>
        </div>
        <div className="mt-2 flex gap-2">
          <FieldInput value={draftCustom.model_id} onChange={(e) => setDraftCustom({ ...draftCustom, model_id: e.target.value })} className="font-mono" placeholder="model-id or vendor/model" />
          <button type="button" onClick={addCustomModel} disabled={!draftCustom.label.trim() || !draftCustom.model_id.trim()} className="shrink-0 rounded-lg border border-pm-accent/50 bg-pm-accent/10 px-3 py-2 text-sm text-pm-accent hover:bg-pm-accent/20 disabled:opacity-50">Add</button>
        </div>
      </SettingBlock>
      <InfoCard title="Shared tool surface" description="Sidebar API loop and external MCP hosts use the same list, spawn, read, write, kill, snapshot, approve, and route bridge tools." />
    </SettingsSection>
  );
}
