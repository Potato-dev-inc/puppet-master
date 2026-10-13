// One inline icon set: 16px, 1.5px stroke, round caps.
import type { ReactNode } from 'react';

function Icon({ children, size = 16 }: { children: ReactNode; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export const IconPlus = () => (
  <Icon>
    <path d="M8 3v10M3 8h10" />
  </Icon>
);
export const IconInbox = () => (
  <Icon>
    <path d="M2.5 9.5 4 3.5h8l1.5 6M2.5 9.5v3h11v-3M2.5 9.5h3.2l.8 1.5h3l.8-1.5h3.2" />
  </Icon>
);
export const IconMore = () => (
  <Icon>
    <circle cx="3.5" cy="8" r=".6" fill="currentColor" />
    <circle cx="8" cy="8" r=".6" fill="currentColor" />
    <circle cx="12.5" cy="8" r=".6" fill="currentColor" />
  </Icon>
);
export const IconSearch = () => (
  <Icon>
    <circle cx="7" cy="7" r="4" />
    <path d="m10 10 3.5 3.5" />
  </Icon>
);
export const IconSettings = () => (
  <Icon>
    <circle cx="8" cy="8" r="2" />
    <path d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6M3.6 3.6l1.1 1.1M11.3 11.3l1.1 1.1M3.6 12.4l1.1-1.1M11.3 4.7l1.1-1.1" />
  </Icon>
);
export const IconSun = () => (
  <Icon>
    <circle cx="8" cy="8" r="2.6" />
    <path d="M8 1.5v1.4M8 13.1v1.4M1.5 8h1.4M13.1 8h1.4M3.4 3.4l1 1M11.6 11.6l1 1M3.4 12.6l1-1M11.6 4.4l1-1" />
  </Icon>
);
export const IconMoon = () => (
  <Icon>
    <path d="M13 9.6A5.5 5.5 0 0 1 6.4 3a5.5 5.5 0 1 0 6.6 6.6Z" />
  </Icon>
);
export const IconTerminal = () => (
  <Icon>
    <rect x="2" y="3" width="12" height="10" rx="2" />
    <path d="m5 7 2 1.5L5 10M8.5 10.5H11" />
  </Icon>
);
export const IconStop = () => (
  <Icon>
    <rect x="3.5" y="3.5" width="9" height="9" rx="2" />
  </Icon>
);
export const IconCopy = () => (
  <Icon>
    <rect x="5.5" y="5.5" width="8" height="8" rx="2" />
    <path d="M10.5 5.5v-1a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v4a2 2 0 0 0 2 2h1" />
  </Icon>
);
export const IconReflow = () => (
  <Icon>
    <path d="M13 8a5 5 0 1 1-1.5-3.5M13 2.5v2.8h-2.8" />
  </Icon>
);
export const IconClose = () => (
  <Icon>
    <path d="m4 4 8 8M12 4l-8 8" />
  </Icon>
);
export const IconLayers = () => (
  <Icon>
    <path d="m8 2.5 6 3-6 3-6-3 6-3ZM2 8.5l6 3 6-3M2 11l6 3 6-3" />
  </Icon>
);
export const IconPane = () => (
  <Icon>
    <rect x="2.5" y="3" width="11" height="10" rx="2" />
    <path d="M2.5 6.5h11" />
  </Icon>
);
export const IconRename = () => (
  <Icon>
    <path d="m10.5 3.5 2 2M3 13l.6-2.6 7-7a1.4 1.4 0 0 1 2 2l-7 7L3 13Z" />
  </Icon>
);
