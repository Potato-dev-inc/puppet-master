import type { CockpitApi } from '../../hooks/useCockpit';
import { projectWithCopy } from './shell-labels';
import { stuckTitle, useLoopGuard, type LoopSignal } from '../../lib/loop-guard';
import '../../styles/loop-guard.css';
import { CardButton, InboxCard } from './InboxCard';

/** Window event that opens the sidebar "+" menu (the Sidebar listens for it). */
export const OPEN_NEW_MENU = 'pm:open-new-menu';

export function focusTerminal(): void {
  window.setTimeout(() => {
    if (typeof document === 'undefined') return; // the window was torn down (tests, closing)
    const slot = document.querySelector<HTMLElement>('.ck-term-slot');
    (slot?.querySelector<HTMLElement>('[tabindex], textarea, canvas') ?? slot)?.focus();
  }, 60);
}

/** An InboxCard: what looks wrong, the lines that show it, and four ways out. */
export function StuckCard({
  name,
  project,
  signal,
  onHint,
  onInterrupt,
  onAsk,
  onDismiss,
}: {
  name: string;
  project?: string;
  signal: LoopSignal;
  onHint: () => void;
  onInterrupt: () => void;
  onAsk: () => void;
  onDismiss: () => void;
}) {
  return (
    <InboxCard
      kind="stuck"
      glyph="needs"
      title={stuckTitle(name, signal)}
      meta={project}
      data={{ 'data-stuck': '' }}
      body={
        signal.evidence_lines.length > 0 ? (
          <ul className="ib-evidence">
            {signal.evidence_lines.map((line, i) => (
              <li key={i}>{line}</li>
            ))}
          </ul>
        ) : null
      }
      actions={
        <>
          <CardButton tone="primary" role="answer" onClick={onHint}>
            Give it a hint
          </CardButton>
          <CardButton tone="secondary" role="answer" onClick={onInterrupt}>
            Interrupt
          </CardButton>
          <CardButton tone="secondary" role="answer" onClick={onAsk}>
            Ask another agent
          </CardButton>
        </>
      }
      more={
        <CardButton tone="plain" role="dismiss" onClick={onDismiss}>
          Dismiss
        </CardButton>
      }
    />
  );
}

/**
 * The card for one pane, wired to the cockpit. Renders nothing when the pane is not stuck. `dock`
 * is for the pane view, which docks the card with its own margin; in the Inbox the list spaces it.
 */
export function PaneStuckCard({ c, paneId, dock = true }: { c: CockpitApi; paneId: string; dock?: boolean }) {
  const { signals, dismiss } = useLoopGuard();
  const signal = signals[paneId];
  if (!signal) return null;
  const name = c.names.get(paneId) ?? paneId;
  const cwd = c.registry.paneList.find((p) => p.info.id === paneId)?.info.cwd;
  const card = (
    <StuckCard
      name={name}
      project={cwd ? projectWithCopy(cwd) : undefined}
      signal={signal}
      onHint={() => {
        if (c.selectedId !== paneId) c.select(paneId);
        focusTerminal();
      }}
      onInterrupt={() => void c.interrupt(paneId)}
      onAsk={() => window.dispatchEvent(new CustomEvent(OPEN_NEW_MENU))}
      onDismiss={() => dismiss(paneId)}
    />
  );
  return dock ? <div className="ib-dock">{card}</div> : card;
}
