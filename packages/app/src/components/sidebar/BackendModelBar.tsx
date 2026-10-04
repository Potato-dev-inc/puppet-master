// Backend + model pickers (and the MCP configured note for CLI backends) under the sidebar title.
import {
  DEFAULT_LLM_MODELS,
  ORCHESTRATOR_BACKEND_LABELS,
  modelKey,
  type LlmModel,
  type OrchestratorBackend,
} from '@puppet-master/shared';
import type { CliOrchestratorBackend } from '../../lib/orchestrator-panes';

interface Props {
  backend: OrchestratorBackend;
  cliBackend: CliOrchestratorBackend | null;
  model: LlmModel;
  availableModels: LlmModel[];
  mcpStatus: string | null;
  onBackendChange: (next: OrchestratorBackend) => void;
  onModelChange: (next: LlmModel) => void;
}

const isPreset = (m: LlmModel) =>
  DEFAULT_LLM_MODELS.some((p) => p.provider === m.provider && p.model_id === m.model_id);

export function BackendModelBar({
  backend,
  cliBackend,
  model,
  availableModels,
  mcpStatus,
  onBackendChange,
  onModelChange,
}: Props) {
  const presetModels = availableModels.filter(isPreset);
  const customModels = availableModels.filter((m) => !isPreset(m));

  return (
    <div className="flex flex-col gap-1 px-3 py-1 border-b border-pm-border text-xs">
      <div className="flex items-center gap-2">
        <span className="text-pm-muted shrink-0">Backend:</span>
        <select
          value={backend}
          onChange={(e) => onBackendChange(e.target.value as OrchestratorBackend)}
          className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-1 py-0.5 text-xs"
        >
          {(Object.keys(ORCHESTRATOR_BACKEND_LABELS) as OrchestratorBackend[]).map((b) => (
            <option key={b} value={b}>
              {ORCHESTRATOR_BACKEND_LABELS[b]}
            </option>
          ))}
        </select>
      </div>
      {backend === 'api' && (
        <div className="flex items-center gap-2">
          <span className="text-pm-muted shrink-0">Model:</span>
          <select
            value={modelKey(model)}
            onChange={(e) => {
              const found = availableModels.find((m) => modelKey(m) === e.target.value);
              if (found) onModelChange(found);
            }}
            className="flex-1 min-w-0 bg-pm-bg border border-pm-border rounded px-1 py-0.5 text-xs"
          >
            <optgroup label="Built-in">
              {presetModels.map((m) => (
                <option key={modelKey(m)} value={modelKey(m)}>
                  {m.label}
                </option>
              ))}
            </optgroup>
            {customModels.length > 0 && (
              <optgroup label="Custom">
                {customModels.map((m) => (
                  <option key={modelKey(m)} value={modelKey(m)}>
                    {m.label}
                  </option>
                ))}
              </optgroup>
            )}
          </select>
        </div>
      )}
      {cliBackend && mcpStatus && (
        <div className="text-[10px] text-pm-muted truncate" title={mcpStatus}>
          {mcpStatus}
        </div>
      )}
    </div>
  );
}
