// One labelled card of the Advanced tab: a title, an optional line under it, then the controls.
import type { ReactNode } from 'react';

export function AdvancedSection({ title, line, children, id }: { title: string; line?: string; children: ReactNode; id?: string }) {
  return (
    <section className="ck-potato-adv-section" aria-labelledby={id} aria-label={id ? undefined : title}>
      <h3 className="ck-potato-adv-title" id={id}>
        {title}
      </h3>
      {line && <p className="ck-potato-adv-line">{line}</p>}
      {children}
    </section>
  );
}
