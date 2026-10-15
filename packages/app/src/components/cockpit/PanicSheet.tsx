import type { CockpitApi } from '../../hooks/useCockpit';
import type { StopReport } from '../../lib/loop-guard';
import { Sheet } from './Sheet';
import { projectWithCopy } from './shell-labels';
import '../../styles/loop-guard.css';

/** "Stopped 2 agents": what the panic button interrupted. */
export function PanicSheet({
  report,
  names,
  onClose,
}: {
  report: StopReport;
  names: CockpitApi['names'];
  onClose: () => void;
}) {
  const n = report.stopped.length;
  const title = n === 0 ? 'Nothing was running' : n === 1 ? 'Stopped 1 agent' : `Stopped ${n} agents`;
  return (
    <Sheet className="ck-panic" labelledBy="ck-panic-t" onClose={onClose}>
        <h2 className="ck-section" id="ck-panic-t">
          {title}
        </h2>
        {n === 0 && report.failed.length === 0 && <p className="ck-sub">No agent was working, so nothing needed stopping.</p>}
        {n > 0 && (
          <>
            <p className="ck-sub">Each one was stopped. Their conversations are still open.</p>
            <ul className="ck-panic-list">
              {report.stopped.map((p) => (
                <li key={p.pane_id}>
                  <span className="ck-panic-name">{names.get(p.pane_id) ?? p.agent_type}</span>
                  <span className="ck-panic-sub">{projectWithCopy(p.cwd)}</span>
                </li>
              ))}
            </ul>
          </>
        )}
        {report.failed.length > 0 && (
          <>
            <p className="ck-sub">Could not stop:</p>
            <ul className="ck-panic-list">
              {report.failed.map(([id, reason]) => (
                <li key={id}>
                  <span className="ck-panic-name">{names.get(id) ?? id}</span>
                  <span className="ck-panic-sub">{reason}</span>
                </li>
              ))}
            </ul>
          </>
        )}
        <div className="ck-sheet-actions">
          <button type="button" className="ck-btn ck-btn--primary" autoFocus onClick={onClose}>
            Done
          </button>
        </div>
    </Sheet>
  );
}
