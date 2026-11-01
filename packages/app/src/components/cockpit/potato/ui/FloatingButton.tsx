// A round-cornered icon button that floats on the thread (the header's panel button, later Jump to latest). `pressed`
// is for a button that shows a state, like the open panel.
import type { ReactNode } from 'react';
import '../../../../styles/cockpit-potato-ui.css';

export interface FloatingButtonProps {
  label: string;
  title?: string;
  pressed?: boolean;
  onClick: () => void;
  className?: string;
  children: ReactNode;
}

export function FloatingButton({ label, title, pressed, onClick, className = '', children }: FloatingButtonProps) {
  return (
    <button type="button" className={`ck-potato-float ${className}`.trim()} aria-label={label} title={title ?? label} aria-pressed={pressed} onClick={onClick}>
      {children}
    </button>
  );
}
