// A short state word in a pill. No colour: "strong" is the inverted fill (it needs you, it is running), "quiet" is an
// outline (done, you have control) and "dim" is a faint outline (skipped). Section 4.0 of the UI map.
import type { ReactNode } from 'react';
import '../../../../styles/cockpit-potato-ui.css';

export type PillTone = 'strong' | 'quiet' | 'dim';

export function StatusPill({ tone = 'quiet', children }: { tone?: PillTone; children: ReactNode }) {
  return <span className={`ck-potato-pill ck-potato-pill--${tone}`}>{children}</span>;
}
