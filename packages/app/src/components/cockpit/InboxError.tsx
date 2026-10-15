// An Inbox error card: what happened, what is safe, and one next step. An InboxCard like the rest.
import { useEffect, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { cockpitApi, type PaneRunInfo } from '../../lib/cockpit-api';
import { CardButton, InboxCard } from './InboxCard';
import { OPEN_NEW_MENU } from './StuckCard';
import { explainError, unknownError } from './error-sentences';

interface Props {
  c: CockpitApi;
  paneId: string;
  agentType: string;
  name: string;
  /** The project's name, next to the title. */
  project?: string;
  /** How long ago it stopped, in ms. */
  age?: number | null;
  onOpenDoctor?: () => void;
}

export function PaneErrorCard({ c, paneId, agentType, name, project, age, onOpenDoctor }: Props) {
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
  const words = known ?? unknownError(name);
  const fix = () => {
    if (!known) return;
    if (known.fix === 'start_again') void c.newAgent(agentType);
    else if (known.fix === 'install') onOpenDoctor?.();
    else c.select(paneId);
  };
  const open = (tone: 'primary' | 'plain') => (
    <CardButton tone={tone} role="open" onClick={() => c.select(paneId)}>
      Open
    </CardButton>
  );
  return (
    <InboxCard
      kind="error"
      glyph={known?.glyph ?? 'error'}
      title={words.title}
      meta={project}
      age={age ?? null}
      reason={<span title={code ?? undefined}>{words.sentence}</span>}
      data={{ 'data-error-pane': paneId }}
      actions={
        <>
          {known && (
            <CardButton tone="primary" role="answer" onClick={fix}>
              {known.fixLabel}
            </CardButton>
          )}
          {known?.fix !== 'open_pane' && open(known ? 'plain' : 'primary')}
        </>
      }
      more={
        <CardButton tone="plain" onClick={() => window.dispatchEvent(new CustomEvent(OPEN_NEW_MENU))}>
          Ask another agent
        </CardButton>
      }
    />
  );
}
