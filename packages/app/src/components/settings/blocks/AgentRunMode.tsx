// Settings > Agents > Agent definitions: "Agent runs" on each custom agent row.
// Visible pane (default) or One-task mode. One-task mode can be picked only when the agent's headless mode is
// verified (the wizard's live test passed for this exact definition); otherwise it is disabled with the reason.
import { useCallback, useEffect, useState } from 'react';
import {
  lapsedNote,
  notVerifiedText,
  ONE_TASK_LABEL,
  tauriAgentRunModeApi,
  VISIBLE_LABEL,
  type AgentRunModeApi,
  type RunMode,
  type RunModeAgent,
} from '../../../lib/agentRunMode';
import { Popup } from '../controls';

export function RunModeControl({
  agentId,
  api = tauriAgentRunModeApi,
}: {
  agentId: string;
  api?: AgentRunModeApi;
}) {
  const [agent, setAgent] = useState<RunModeAgent | null>(null);
  const [error, setError] = useState<string | null>(null);

  const apply = useCallback(
    (view: { agents: RunModeAgent[] }) => {
      setAgent(view.agents.find((a) => a.id === agentId) ?? null);
    },
    [agentId],
  );

  useEffect(() => {
    let alive = true;
    api
      .get()
      .then((view) => alive && apply(view))
      // No desktop shell (a browser preview): there is nothing to choose, so show nothing.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [api, apply]);

  if (!agent) return null;

  const save = async (mode: RunMode) => {
    setError(null);
    try {
      apply(await api.set(agent.id, mode));
    } catch (e) {
      setError(e instanceof Error ? e.message.replace(/^RUN_MODE_NOT_VERIFIED:\s*/, '') : String(e));
    }
  };

  const reason = agent.headless_verified ? '' : notVerifiedText(agent.headless_verified_reason);
  const lapsed = lapsedNote(agent);
  return (
    <div data-testid="run-mode" className="mt-2 flex flex-wrap items-center gap-2 text-xs">
      <label htmlFor={`run-mode-${agent.id}`} className="font-medium">
        Agent runs
      </label>
      <Popup
        id={`run-mode-${agent.id}`}
        aria-label={`How ${agent.label ?? agent.id} runs work`}
        value={agent.run_mode}
        onChange={(event) => void save(event.target.value as RunMode)}
      >
        <option value="visible">{VISIBLE_LABEL}</option>
        <option value="one_task" disabled={!agent.headless_verified}>
          {ONE_TASK_LABEL}
        </option>
      </Popup>
      {reason && (
        <span data-testid="run-mode-reason" className="text-pm-muted">
          {reason}
        </span>
      )}
      {lapsed && (
        <span data-testid="run-mode-lapsed" className="text-pm-muted">
          {lapsed}
        </span>
      )}
      {error && (
        <span role="alert" className="text-pm-text">
          {error}
        </span>
      )}
    </div>
  );
}
