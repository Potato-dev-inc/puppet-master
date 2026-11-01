// One labelled block of the bot panel's Advanced tab: a title, an optional action at its right, and a rounded card
// around the content. Shared by Notifications, Open work and Memory.
import type { ReactNode } from 'react';
import '../../../../styles/cockpit-potato-settings.css';

export interface PanelSectionProps {
  /** Makes the heading's id unique on the page: `${id}-title`. */
  id: string;
  title: string;
  action?: ReactNode;
  children: ReactNode;
}

export function PanelSection({ id, title, action, children }: PanelSectionProps) {
  return (
    <section className="ck-potato-section" aria-labelledby={`${id}-title`}>
      <header className="ck-potato-section-head">
        <h3 className="ck-potato-section-title" id={`${id}-title`}>
          {title}
        </h3>
        {action}
      </header>
      <div className="ck-potato-section-card">{children}</div>
    </section>
  );
}
