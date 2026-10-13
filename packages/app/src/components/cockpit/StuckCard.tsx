import type { CockpitApi } from '../../hooks/useCockpit';
import { stuckTitle, useLoopGuard, type LoopSignal } from '../../lib/loop-guard';
import '../../styles/loop-guard.css';

/** Window event that opens the sidebar "+" menu (the Sidebar listens for it). */
export const OPEN_NEW_MENU = 'pm:open-new-menu';

export function focusTerminal(): void {
  window.setTimeout(() => {
    if (typeof document === 'undefined') return; // the window was torn down (tests, closing)
    const slot = document.querySelector<HTMLElement>('.ck-term-slot');
    (slot?.querySelector<HTMLElement>('[tabindex], textarea, canvas') ?? slot)?.focus();
  }, 60);
}

/** Quiet card: what looks wrong, the lines that show it, and four ways out. */
export function StuckCard({
  name,
  signal,
  onHint,
  onInterrupt,
  onAsk,
  onDismiss,
}: {
  name: string;
  signal: LoopSignal;
  onHint: () => void;
  onInterrupt: () => void;
  onAsk: () => void;
  onDismiss: () => void;
}) {
  return (
    <section className="ck-stuck" role="group" aria-label="Agent seems stuck">
      <p className="ck-stuck-title">{stuckTitle(name, signal)}</p>
      {signal.evidence_lines.length > 0 && (
        <ul className="ck-stuck-evidence">
          {signal.evidence_lines.map((line, i) => (
            <li key={i}>{line}</li>
          ))}
        </ul>
      )}
      <div className="ck-stuck-actions">
        <button type="button" className="ck-btn ck-btn--sm ck-btn--primary" onClick={onHint}>
          Give it a hint
        </button>
        <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={onInterrupt}>
          Interrupt
        </button>
        <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={onAsk}>
          Ask another agent
        </button>
        <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" onClick={onDismiss}>
          Dismiss
        </button>
      </div>
    </section>
  );
}

/** The card for one pane, wired to the cockpit. Renders nothing when the pane is not stuck. */
export function PaneStuckCard({ c, paneId }: { c: CockpitApi; paneId: string }) {
  const { signals, dismiss } = useLoopGuard();
  const signal = signals[paneId];
  if (!signal) return null;
  const name = c.names.get(paneId) ?? paneId;
  return (
    <StuckCard
      name={name}
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
}
