// Overview "Agents" card: every registered agent with its source, state and modes.
import { useAgentRegistry, type AgentDescriptor } from '../lib/agentRegistry';

export function agentStateLabel(agent: AgentDescriptor): string {
  if (!agent.installed) return 'not installed';
  if (!agent.dispatchable && agent.not_dispatchable_reason) return 'not dispatchable';
  return 'ready';
}

export function agentModesLabel(agent: AgentDescriptor): string {
  const modes = [agent.modes.visible ? 'visible' : '', agent.modes.headless ? 'headless' : ''].filter(Boolean);
  return modes.length ? modes.join(' + ') : 'none';
}

export function AgentsOverviewCard({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { agents } = useAgentRegistry();
  return (
    <section className="pm-home-panel" data-testid="agents-card">
      <div className="pm-home-panel-head">
        <h2>Agents</h2>
        <p>{agents.length} registered. New ones appear here as soon as you approve them.</p>
      </div>
      <ul className="pm-home-pane-list">
        {agents.map((agent) => (
          <li key={agent.id} data-testid="agent-row" className="pm-home-pane-row" title={agent.not_dispatchable_reason ?? undefined}>
            <span className="pm-home-pane-main">
              <span className="pm-home-pane-title">
                {agent.label || agent.id}
                {agent.experimental ? ' (experimental)' : ''}
              </span>
              <span className="pm-home-pane-meta">
                {agent.source} - {agentModesLabel(agent)}
              </span>
            </span>
            <span className="pm-home-pane-badge">{agentStateLabel(agent)}</span>
          </li>
        ))}
      </ul>
      <div className="pm-home-empty">
        <button type="button" onClick={onOpenSettings} className="pm-home-btn pm-home-btn--ghost">
          Manage in Settings &gt; Agents
        </button>
      </div>
    </section>
  );
}
