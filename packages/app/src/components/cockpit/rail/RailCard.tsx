// One card of the session rail: a small heading (with an optional note on the right) and the card's lines.
import type { ReactNode } from 'react';

export function RailCard({ title, note, testId, children }: { title: string; note?: ReactNode; testId?: string; children: ReactNode }) {
  return (
    <section className="ck-rail-card" aria-label={title} data-testid={testId}>
      <h3 className="ck-rail-h">
        <span>{title}</span>
        {note != null && <span className="ck-rail-note">{note}</span>}
      </h3>
      {children}
    </section>
  );
}
