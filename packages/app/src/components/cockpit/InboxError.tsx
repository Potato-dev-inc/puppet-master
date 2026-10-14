// Body of an Inbox error card: why the agent stopped, in one sentence, and one fix button.
import { useEffect, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { cockpitApi, type PaneRunInfo } from '../../lib/cockpit-api';
import { OPEN_NEW_MENU } from './StuckCard';
import { explainError, unknownErrorSentence } from './error-sentences';

interface Props {
  c: CockpitApi;
  paneId: string;
  agentType: string;
  name: string;
  onOpenDoctor?: () => void;
}

export function PaneErrorBody({ c, paneId, agentType, name, onOpenDoctor }: Props) {
  const [run, setRun] = useState<PaneRunInfo | null>(null);
  useEffect(() => {
    let alive = true;
    cockpitApi
      .paneRunInfo(paneId)
      .then((info) => alive && setRun(info))
      .catch(() => undefined); // no run info: the card falls back to the general sentence
    return () => {
      alive = false;
    };
  }, [paneId]);

  const code = run?.error?.code ?? null;
  const known = explainError(code, name);
  const fix = () => {
    if (!known) return;
    if (known.fix === 'start_again') void c.newAgent(agentType);
    else if (known.fix === 'install') onOpenDoctor?.();
    else c.select(paneId);
  };
  return (
    <>
      <p className="ck-inbox-q" title={code ?? undefined}>
        {known ? known.sentence : unknownErrorSentence(name)}
      </p>
      <div className="ck-inbox-actions">
        {known && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={fix}>
            {known.fixLabel}
          </button>
        )}
        {known?.fix !== 'open_pane' && (
          <button type="button" className={`ck-btn ${known ? 'ck-btn--plain' : 'ck-btn--secondary'}`} onClick={() => c.select(paneId)}>
            Open
          </button>
        )}
        <button type="button" className="ck-btn ck-btn--plain" onClick={() => window.dispatchEvent(new CustomEvent(OPEN_NEW_MENU))}>
          Ask another agent
        </button>
      </div>
    </>
  );
}
