// Settings > Orchestrator & Models: which brain steers the agents, the librarian and OpenCode defaults,
// the API orchestrator's provider and model, and the custom models the picker offers. Every control autosaves.
import { useState } from 'react';
import { modelKey, ORCHESTRATOR_BACKEND_LABELS, type LlmModel, type LlmProvider, type OrchestratorBackend } from '@puppet-master/shared';
import { listModels } from '../../../lib/settings';
import { TextControl } from '../controls';
import type { RowDef, SettingsEnv } from '../model';
import { Btn, FieldInput, FieldSelect, SettingBlock, SettingRow } from '../settings-ui';

const PROVIDERS: { value: LlmProvider; label: string }[] = [
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'openrouter', label: 'OpenRouter' },
];

const BACKENDS = Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[];

const trim = (text: string) => text.trim();

interface CustomDraft {
  provider: LlmProvider;
  label: string;
  model_id: string;
}

/**
 * The custom models of the current provider, with Remove, and the add form. The add form's fields are
 * local draft state: nothing is saved until Add is pressed.
 */
function CustomModels({ env }: { env: SettingsEnv }) {
  const { settings } = env;
  const custom = settings.custom_models ?? [];
  const forProvider = custom.filter((m) => m.provider === settings.default_provider);
  const [draft, setDraft] = useState<CustomDraft>(() => ({ provider: settings.default_provider, label: '', model_id: '' }));
  const ready = draft.label.trim() !== '' && draft.model_id.trim() !== '';

  const remove = (target: LlmModel) => {
    const remaining = custom.filter((m) => !(m.provider === target.provider && m.model_id === target.model_id));
    void env.patch({ custom_models: remaining });
  };

  const add = async () => {
    if (!ready) return;
    const entry: LlmModel = { provider: draft.provider, model_id: draft.model_id.trim(), label: draft.label.trim() };
    const next = [...custom.filter((m) => !(m.provider === entry.provider && m.model_id === entry.model_id)), entry];
    try {
      await env.patch({ custom_models: next, default_provider: entry.provider, default_model: entry.model_id });
      setDraft((d) => ({ ...d, label: '', model_id: '' }));
    } catch {
      // The panel shows the error on this row. The draft stays so the same entry can be sent again.
    }
  };

  return (
    <>
      {forProvider.length > 0 && (
        <ul className="mb-3 space-y-2">
          {forProvider.map((m) => (
            <li key={modelKey(m)} className="flex items-center gap-2 text-xs">
              <span className="min-w-0 flex-1 truncate">
                {m.label} — <span className="font-mono">{m.model_id}</span>
              </span>
              <Btn small aria-label={`Remove ${m.label}`} onClick={() => remove(m)}>
                Remove
              </Btn>
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <FieldSelect aria-label="Custom model provider" value={draft.provider} onChange={(e) => setDraft((d) => ({ ...d, provider: e.target.value as LlmProvider }))}>
          {PROVIDERS.map((p) => (
            <option key={p.value} value={p.value}>
              {p.label}
            </option>
          ))}
        </FieldSelect>
        <FieldInput
          aria-label="Custom model display label"
          className="min-w-0 flex-1"
          placeholder="My model"
          autoComplete="off"
          value={draft.label}
          onChange={(e) => setDraft((d) => ({ ...d, label: e.target.value }))}
        />
        <FieldInput
          aria-label="Custom model ID"
          className="min-w-0 flex-1 font-mono"
          placeholder="model-id or vendor/model"
          autoComplete="off"
          spellCheck={false}
          value={draft.model_id}
          onChange={(e) => setDraft((d) => ({ ...d, model_id: e.target.value }))}
        />
        <Btn primary disabled={!ready} onClick={() => void add()}>
          Add
        </Btn>
      </div>
    </>
  );
}

export const modelsRows: RowDef[] = [
  {
    id: 'orchestrator-backend',
    label: 'Default orchestrator',
    hint: 'The brain that routes work and delegates to your agents.',
    keywords: 'backend brain claude claude code codex codex cli opencode opencode cli api llm',
    group: 'Orchestrator',
    render: (env) => (
      <SettingRow label="Default orchestrator" description="The brain that routes work and delegates to your agents.">
        <FieldSelect
          aria-label="Default orchestrator"
          value={env.settings.orchestrator_backend ?? 'api'}
          onChange={(e) => void env.patch({ orchestrator_backend: e.target.value as OrchestratorBackend })}
        >
          {BACKENDS.map((backend) => (
            <option key={backend} value={backend}>
              {ORCHESTRATOR_BACKEND_LABELS[backend]}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>
    ),
  },
  {
    id: 'librarian-indexer',
    label: 'Librarian indexer',
    hint: 'Leave blank to use the OpenCode librarian. Or enter the path to scripts/project-indexer.py for a static-only index.',
    keywords: 'librarian indexer path script static project-indexer opencode',
    group: 'Orchestrator',
    render: (env) => (
      <SettingRow
        label="Librarian indexer"
        description="Leave blank to use the OpenCode librarian. Or enter the path to scripts/project-indexer.py for a static-only index."
      >
        <TextControl
          aria-label="Librarian indexer"
          mono
          placeholder="opencode"
          value={env.settings.librarian_indexer_path ?? ''}
          normalize={trim}
          onCommit={(text) => env.patch({ librarian_indexer_path: text || null })}
        />
      </SettingRow>
    ),
  },
  {
    id: 'opencode-provider',
    label: 'OpenCode provider',
    hint: 'Used when a prompt to an OpenCode agent does not name a model.',
    keywords: 'opencode provider id anthropic model',
    group: 'OpenCode',
    render: (env) => (
      <SettingRow label="OpenCode provider" description="Used when a prompt to an OpenCode agent does not name a model.">
        <TextControl
          aria-label="OpenCode provider ID"
          mono
          placeholder="anthropic"
          value={env.settings.opencode_model_provider ?? ''}
          normalize={trim}
          onCommit={(text) => env.patch({ opencode_model_provider: text || null })}
        />
      </SettingRow>
    ),
  },
  {
    id: 'opencode-model',
    label: 'OpenCode model',
    hint: 'The model used with the provider above.',
    keywords: 'opencode model id',
    group: 'OpenCode',
    render: (env) => (
      <SettingRow label="OpenCode model" description="The model used with the provider above.">
        <TextControl
          aria-label="OpenCode model ID"
          mono
          placeholder="claude-sonnet-4"
          value={env.settings.opencode_model_id ?? ''}
          normalize={trim}
          onCommit={(text) => env.patch({ opencode_model_id: text || null })}
        />
      </SettingRow>
    ),
  },
  {
    id: 'default-provider',
    label: 'Provider',
    hint: 'Which service the API orchestrator sends requests to.',
    keywords: 'provider api openai anthropic openrouter default',
    group: 'Model for the API orchestrator',
    render: (env) => (
      <SettingRow label="Provider" description="Which service the API orchestrator sends requests to.">
        <FieldSelect
          aria-label="Default provider"
          value={env.settings.default_provider}
          onChange={(e) => {
            const provider = e.target.value as LlmProvider;
            const first = listModels(env.settings).find((m) => m.provider === provider);
            void env.patch({ default_provider: provider, default_model: first?.model_id ?? env.settings.default_model });
          }}
        >
          {PROVIDERS.map((p) => (
            <option key={p.value} value={p.value}>
              {p.label}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>
    ),
  },
  {
    id: 'default-model',
    label: 'Model',
    hint: 'The model the API orchestrator uses.',
    keywords: 'model api default openrouter openai claude gpt',
    group: 'Model for the API orchestrator',
    render: (env) => (
      <SettingRow label="Model" description="The model the API orchestrator uses.">
        <FieldSelect
          aria-label="Default model"
          value={env.settings.default_model}
          onChange={(e) => void env.patch({ default_model: e.target.value })}
        >
          {listModels(env.settings)
            .filter((m) => m.provider === env.settings.default_provider)
            .map((m) => (
              <option key={modelKey(m)} value={m.model_id}>
                {m.label}
              </option>
            ))}
        </FieldSelect>
      </SettingRow>
    ),
  },
  {
    id: 'custom-models',
    label: 'Custom models',
    hint: 'Add your own vendor/model ids to the model picker.',
    keywords: 'custom model provider display label model id add remove openrouter',
    group: 'Model for the API orchestrator',
    render: (env) => (
      <SettingBlock label="Custom models" description="Add your own vendor/model ids to the model picker.">
        <CustomModels env={env} />
      </SettingBlock>
    ),
  },
];
