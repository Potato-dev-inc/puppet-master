// "Agent" tab: the CLI orchestrator terminal, or the API chat transcript, plus the busy/interrupt strip.
import type { PaneData } from '../../hooks/usePaneRegistry';
import type { PaneTunnelApi } from '../../hooks/usePaneTunnel';
import type { CliOrchestratorBackend } from '../../lib/orchestrator-panes';
import { OrchestratorTerminal } from '../OrchestratorTerminal';
import type { UiMessage } from './chat-model';

interface Props {
  cliBackend: CliOrchestratorBackend | null;
  projectPath: string | null;
  orchestratorPane: PaneData | undefined;
  orchestratorStarting: boolean;
  orchestratorError: string | null;
  orchestratorTunnel: PaneTunnelApi;
  onRetryOrchestrator: () => void;
  messages: UiMessage[];
  busy: boolean;
  standbyPanes: Array<{ id: string; status: string }>;
  onInterrupt: () => void;
}

export function AgentView({
  cliBackend,
  projectPath,
  orchestratorPane,
  orchestratorStarting,
  orchestratorError,
  orchestratorTunnel,
  onRetryOrchestrator,
  messages,
  busy,
  standbyPanes,
  onInterrupt,
}: Props) {
  return (
    <>
      {cliBackend ? (
        projectPath ? (
          <OrchestratorTerminal
            backend={cliBackend}
            pane={orchestratorPane}
            starting={orchestratorStarting}
            error={orchestratorError}
            subscribePaneData={orchestratorTunnel.subscribePaneData}
            transport={orchestratorTunnel.transport}
            syncPTYResize
            onRetry={onRetryOrchestrator}
          />
        ) : (
          <div className="flex-1 min-h-0 flex items-center justify-center p-4 text-xs text-center text-pm-muted">
            Pick a project folder in the header to start the orchestrator agent.
          </div>
        )
      ) : (
        <div className="flex-1 min-h-0 overflow-auto p-3 space-y-2">
          {messages.length === 0 && (
            <div className="text-xs text-pm-muted">
              Ask the Puppet Master to coordinate your panes. Try &quot;spawn a claude pane and ask it to summarize the repo&quot;.
              Add custom models in Settings.
            </div>
          )}
          {messages.map((m) => (
            <div
              key={m.id}
              className={`text-xs whitespace-pre-wrap ${
                m.role === 'user'
                  ? 'text-zinc-100'
                  : m.role === 'assistant'
                  ? 'text-zinc-200'
                  : 'text-pm-warn'
              }`}
            >
              <span className="text-pm-muted mr-1">[{m.role}]</span>
              {m.text}
            </div>
          ))}
        </div>
      )}
      {busy && (
        <div className="border-t border-pm-border px-3 py-2 text-[10px] text-pm-muted">
          {standbyPanes.length > 0 ? (
            <div className="truncate" title={standbyPanes.map((p) => p.id).join(', ')}>
              Standing by for: {standbyPanes.map((p) => p.id).join(', ')}
            </div>
          ) : (
            <div>Orchestrator busy.</div>
          )}
          <button
            type="button"
            onClick={onInterrupt}
            className="mt-1 px-2 py-1 text-xs rounded border border-pm-err/50 bg-pm-err/10 text-pm-err hover:bg-pm-err/20"
          >
            Interrupt
          </button>
        </div>
      )}
    </>
  );
}
