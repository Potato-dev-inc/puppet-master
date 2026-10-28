// The card for a pane that waits on the person: the same card in the Inbox list and docked in the pane's
// Terminal tab. A rated prompt is an ApprovalPrompt card (Allow once / Always allow / Deny, Deny first on
// high risk); a menu Puppet Master could not rate keeps the agent's own words, equal weight, no default.
// Every answer carries the person's click (`lib/user-gesture.ts`): the backend refuses one without it.
import type { CockpitApi } from '../../hooks/useCockpit';
import { ApprovalPrompt } from './ApprovalPrompt';
import { gestureOf } from '../../lib/user-gesture';
import { CardButton, InboxCard } from './InboxCard';
import { promptSummary } from './model';
import { projectWithCopy } from './shell-labels';
import { paneHeading } from './titles';

export function PaneApprovalCard({
  c,
  paneId,
  autoFocus,
  onOpen,
  asks,
  openLabel,
}: {
  c: CockpitApi;
  paneId: string;
  /** High risk: move the keyboard to Deny when the card appears. */
  autoFocus?: boolean;
  /** The "Open terminal" button. Absent in the pane itself, where the terminal is already open. */
  onOpen?: () => void;
  /** A helper's question shown in its parent's Chat: "Claude Code helper asks" leads the title. */
  asks?: string;
  /** The open button's words (default "Open terminal"). */
  openLabel?: string;
}) {
  const pane = c.registry.panes.get(paneId);
  const prompt = c.prompts[paneId];
  const since = c.statusSince.get(paneId);
  const age = since === undefined ? null : c.now - since;
  const agent = c.names.get(paneId) ?? pane?.info.agent_type ?? 'Agent';
  const heading = pane ? paneHeading(pane.info, agent) : { primary: agent, secondary: null };
  const who = [heading.primary, heading.secondary, pane ? projectWithCopy(pane.info.cwd) : undefined].filter(Boolean).join(' · ');
  const plainTitle = (prompt && promptSummary(prompt)) || 'Waiting for your answer';
  const hasChoices = !!prompt && prompt.choices.length > 0;

  if (prompt?.risk && hasChoices) {
    return (
      <ApprovalPrompt
        variant="card"
        paneId={paneId}
        prompt={prompt}
        who={who}
        age={age}
        autoFocus={autoFocus}
        onOpen={onOpen}
        asks={asks}
        openLabel={openLabel}
        onAnswer={(ch, gesture) => void c.answer(paneId, prompt.promptId, ch.id, ch.label, gesture)}
      />
    );
  }
  return (
    <InboxCard
      kind="approval"
      glyph="needs"
      title={asks ? `${asks}: ${plainTitle}` : plainTitle}
      meta={who}
      age={age}
      actions={
        hasChoices ? (
          <>
            {prompt.choices.map((ch) => (
              <CardButton key={ch.id} tone="secondary" role="answer" onClick={(event) => void c.answer(paneId, prompt.promptId, ch.id, ch.label, gestureOf(event))}>
                {ch.label}
              </CardButton>
            ))}
          </>
        ) : null
      }
      more={
        onOpen ? (
          <CardButton tone="plain" role="open" onClick={onOpen}>
            {openLabel ?? 'Open terminal'}
          </CardButton>
        ) : null
      }
    />
  );
}
