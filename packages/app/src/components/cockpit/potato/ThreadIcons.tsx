// The small icons of the message list: tick, alert, chevron, arrow. Same style as ui/Glyph (16 px, 1.5 px stroke, round
// caps); kept here so the thread phase does not edit the shared glyph list.
import type { ReactNode } from 'react';

export type ThreadIconName = 'check' | 'alert' | 'chevron' | 'arrow-down';

const PATHS: Record<ThreadIconName, ReactNode> = {
  check: <path d="M3.5 8.5l3 3 6-7" />,
  alert: (
    <>
      <circle cx="8" cy="8" r="5.75" />
      <path d="M8 5v3.5M8 10.9v.1" />
    </>
  ),
  chevron: <path d="M6.25 4l4 4-4 4" />,
  'arrow-down': <path d="M8 3v9.5M3.75 8.5L8 12.75l4.25-4.25" />,
};

export function ThreadIcon({ name, size = 14 }: { name: ThreadIconName; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {PATHS[name]}
    </svg>
  );
}
