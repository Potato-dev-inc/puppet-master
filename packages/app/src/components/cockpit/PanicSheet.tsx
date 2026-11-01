import { useEffect, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { StopReport } from '../../lib/loop-guard';
import { potatoHoldState, potatoResumeAll, type PotatoHold } from '../../lib/potato-hold';
import { Sheet } from './Sheet';
import '../../styles/loop-guard.css';

function count(n: number, one: string, many: string): string {
  return n === 1 ? `1 ${one}` : `${n} ${many}`;
}

/** "Everything is stopped": what the panic button stopped, paused, held or put to sleep. Rows only for what was running. */
export function PanicSheet({
  report,
  names: _names,
  onClose,
}: {
  report: StopReport;
  names: CockpitApi['names'];
  onClose: () => void;
}) {
  const [hold, setHold] = useState<PotatoHold | null>(null);
  useEffect(() => {
    let live = true;
    (async () => {
      try {
        const h = await potatoHoldState();
        if (live) setHold(h);
      } catch {
        // No desktop runtime (tests, the browser preview): the sheet shows the agent rows only.
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  const agents = report.stopped.length;
  const rows: string[] = [];
  if (agents > 0) rows.push(count(agents, 'agent stopped', 'agents stopped'));
  if (hold && hold.botsPaused > 0) rows.push(count(hold.botsPaused, 'bot paused', 'bots paused'));
  if (hold && hold.routinesHeld > 0) rows.push(count(hold.routinesHeld, 'routine on hold', 'routines on hold'));
  if (hold?.computerAsleep) rows.push("Fish's computer is asleep");

  const resume = async () => {
    try {
      setHold(await potatoResumeAll());
    } catch {
      // The hold stays on; the sheet still says so.
    }
  };

  return (
    <Sheet className="ck-panic" labelledBy="ck-panic-t" onClose={onClose}>
      <h2 className="ck-section" id="ck-panic-t">
        Everything is stopped
      </h2>
      {rows.length === 0 && report.failed.length === 0 && <p className="ck-sub">Nothing was running.</p>}
      {rows.length > 0 && (
        <ul className="ck-panic-list">
          {rows.map((row) => (
            <li key={row}>
              <span className="ck-panic-name">{row}</span>
            </li>
          ))}
        </ul>
      )}
      {report.failed.length > 0 && (
        <>
          <p className="ck-sub">Could not stop:</p>
          <ul className="ck-panic-list">
            {report.failed.map(([id, reason]) => (
              <li key={id}>
                <span className="ck-panic-name">{id}</span>
                <span className="ck-panic-sub">{reason}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      <div className="ck-sheet-actions">
        {hold?.held && (
          <button type="button" className="ck-btn" onClick={() => void resume()}>
            Start bots again
          </button>
        )}
        <button type="button" className="ck-btn ck-btn--primary" autoFocus onClick={onClose}>
          Done
        </button>
      </div>
    </Sheet>
  );
}
