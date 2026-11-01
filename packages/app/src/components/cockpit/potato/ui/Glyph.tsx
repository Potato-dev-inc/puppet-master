// The small icons of the Potato Bot page, in the cockpit's icon style (16 px, 1.5 px stroke, round caps).
import type { ReactNode } from 'react';

export type GlyphName = 'monitor' | 'close' | 'list' | 'chevron-left' | 'chevron-right' | 'plus';

const PATHS: Record<GlyphName, ReactNode> = {
  monitor: (
    <>
      <rect x="2" y="2.75" width="12" height="8" rx="1.75" />
      <path d="M5.75 13.5h4.5M8 10.75v2.75" />
    </>
  ),
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  plus: <path d="M8 3.5v9M3.5 8h9" />,
  list: (
    <>
      <rect x="2.5" y="3" width="11" height="10" rx="2" />
      <path d="M6.5 3v10" />
    </>
  ),
  'chevron-left': <path d="M9.75 4L5.75 8l4 4" />,
  'chevron-right': <path d="M6.25 4l4 4-4 4" />,
};

export function Glyph({ name, size = 16 }: { name: GlyphName; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {PATHS[name]}
    </svg>
  );
}
