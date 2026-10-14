// "Untested version" chip for a pane header. Shown only while Puppet Master is being careful with
// this agent (its version is outside the tested range, or a check found screens it no longer reads).
import { tauriCanaryApi, useCanary, type CanaryApi } from '../../lib/canary';
import '../../styles/canary.css';

export function UntestedChip({ agent, api = tauriCanaryApi }: { agent: string; api?: CanaryApi }) {
  const { status } = useCanary(api);
  const view = status.agents.find((a) => a.agent === agent);
  if (!view?.chip) return null;
  return (
    <span className="ck-untested-chip" data-testid="untested-chip" title={view.plain}>
      {view.chip}
    </span>
  );
}
