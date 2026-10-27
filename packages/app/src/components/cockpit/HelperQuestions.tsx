// The questions of a pane's helpers, under its Chat: one compact card per helper that waits on a choice,
// "Codex helper asks: Pick a colour". It is the Inbox's own card (PaneApprovalCard): the same choices, the
// same answer route (`c.answer` with the prompt's id, so a stale card is refused) and the person's click as
// the typing permit. The Inbox lists the item once; this is a second place to answer it, and it leaves with it.
import type { CockpitApi } from '../../hooks/useCockpit';
import { helperAsksLabel, helpersAskingFor } from '../../lib/helper-questions';
import { PaneApprovalCard } from './PaneApprovalCard';

/** The helpers of `paneId` that wait on a card with choices right now. */
export function helperQuestionIds(c: CockpitApi, paneId: string): string[] {
  const asking = (id: string) => c.registry.panes.get(id)?.status === 'waiting_input' && (c.prompts[id]?.choices.length ?? 0) > 0;
  return helpersAskingFor(c.registry.paneList, paneId, asking);
}

export function HelperQuestions({ c, ids }: { c: CockpitApi; ids: string[] }) {
  if (ids.length === 0) return null;
  return (
    <div className="ck-dock ck-helper-asks">
      {ids.map((id) => {
        const pane = c.registry.panes.get(id);
        const agent = c.names.get(id) ?? pane?.info.agent_type ?? 'Agent';
        return (
          // The prompt's id is in the key: a new question from the same helper is a new card, never the old one reused.
          <div key={`${id}:${c.prompts[id]?.promptId}`} className="ib-dock" role="group" aria-label={`${agent} helper question`}>
            <PaneApprovalCard c={c} paneId={id} autoFocus={false} asks={helperAsksLabel(agent)} openLabel="Open helper" onOpen={() => c.select(id)} />
          </div>
        );
      })}
    </div>
  );
}
