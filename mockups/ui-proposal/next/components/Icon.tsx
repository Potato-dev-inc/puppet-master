import type { ReactNode } from "react";

const P: Record<string, ReactNode> = {
  plus: <path d="M8 3v10M3 8h10" />,
  gear: <><circle cx="8" cy="8" r="2" /><path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M12.6 3.4l-1.4 1.4M4.8 11.2l-1.4 1.4" /></>,
  search: <><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" /></>,
  more: <><circle cx="3.5" cy="8" r=".6" /><circle cx="8" cy="8" r=".6" /><circle cx="12.5" cy="8" r=".6" /></>,
  up: <path d="M8 13V3M3.5 7.5L8 3l4.5 4.5" />,
  inbox: <path d="M2 9.5l1.7-5.5h8.6L14 9.5v3.5H2zM2 9.5h3.5l.8 1.5h3.4l.8-1.5H14" />,
  check: <path d="M3 8.5l3.2 3L13 4.5" />,
  back: <path d="M10 3L5 8l5 5" />,
  later: <><circle cx="8" cy="8" r="6" /><path d="M8 4.5V8l2.5 1.5" /></>,
  sidebar: <><rect x="2" y="3" width="12" height="10" rx="2" /><path d="M6 3v10" /></>,
  chev: <path d="M4 6l4 4 4-4" />,
  clip: <path d="M13 7.5l-4.8 4.8a2.8 2.8 0 0 1-4-4L9 3.5a1.9 1.9 0 0 1 2.7 2.7L7 11a1 1 0 0 1-1.4-1.4L10 5.2" />,
  steer: <path d="M2 8h9M8 4.5L11.5 8 8 11.5M13.5 3v10" />,
  user: <><circle cx="8" cy="5.5" r="2.5" /><path d="M3 13.5c.8-2.6 3-3.5 5-3.5s4.2.9 5 3.5" /></>,
  bolt: <path d="M9 1.5L3.5 9H8l-1 5.5L12.5 7H8z" />,
  stop: <rect x="4" y="4" width="8" height="8" rx="1.5" />,
  terminal: <><rect x="2" y="3" width="12" height="10" rx="2" /><path d="M5 7l2 1.5L5 10M8.5 10.5H11" /></>,
};

export function Icon({ n, size = 16 }: { n: keyof typeof P | string; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      {P[n]}
    </svg>
  );
}
