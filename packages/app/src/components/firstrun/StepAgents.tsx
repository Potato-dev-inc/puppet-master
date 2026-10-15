// Step 2: the Set up agents list, inline. Install, Sign in and Test are the doctor's own actions; this
// step only adds the sentence that says whether the person can go on.
import type { AgentHealthApi } from '../../lib/agent-health';
import type { AgentInstallApi } from '../../lib/agent-install';
import { AgentDoctor } from '../cockpit/AgentDoctor';
import { T } from './copy';
import { readyAgents, type ReadyAgent } from './model';

export function StepAgents({
  ready,
  api,
  installApi,
  onRows,
  onNotify,
  onSignIn,
  onShowPane,
}: {
  ready: ReadyAgent[];
  api?: AgentHealthApi;
  installApi?: AgentInstallApi;
  onRows: (ready: ReadyAgent[]) => void;
  onNotify: (text: string) => void;
  /** Opens the agent in a window so the person can sign in; the welcome waits as a bar. */
  onSignIn: (agentId: string) => void;
  onShowPane: (paneId: string) => void;
}) {
  return (
    <section className="fr-step" aria-labelledby="fr-agents-t">
      <div className="ck-doto fr-num" aria-hidden="true">02</div>
      <h1 className="ck-title" id="fr-agents-t">{T.agentsTitle}</h1>
      <p className="fr-lead">{T.agentsLead}</p>
      <p className="fr-hint">{T.agentsHowTo}</p>
      <AgentDoctor
        embedded
        api={api}
        installApi={installApi}
        onRows={(rows) => onRows(readyAgents(rows))}
        onNotify={onNotify}
        onOpenAgent={onSignIn}
        onShowPane={onShowPane}
      />
      <p className="fr-status" role="status" data-ready={ready.length > 0 ? 'true' : 'false'}>
        {ready.length > 0 ? T.agentsReady(ready.length) : T.agentsNone}
      </p>
    </section>
  );
}
