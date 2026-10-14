// The modal sheet shell shared by the cockpit's confirm and picker sheets: a scrim with a centred `.ck-sheet`.
// Escape closes it (unless `onClose` is omitted, e.g. while busy), Tab and Shift+Tab stay inside it, a click on
// the scrim closes it, and focus goes back to where it was when the sheet closes.
import { useEffect, useRef, type ReactNode } from 'react';

const FOCUSABLE = 'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function useSheetKeys(onClose: (() => void) | undefined) {
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        if (!closeRef.current) return;
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      if (event.key !== 'Tab') return;
      const box = ref.current;
      if (!box) return;
      const items = [...box.querySelectorAll<HTMLElement>(FOCUSABLE)];
      if (items.length === 0) {
        event.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      if (!box.contains(active)) {
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      if (opener?.isConnected) opener.focus();
    };
  }, []);
  return ref;
}

export function Sheet({
  onClose,
  role = 'alertdialog',
  labelledBy,
  className,
  children,
}: {
  /** Omit to make the sheet unclosable for now (a save is running). */
  onClose?: () => void;
  role?: 'alertdialog' | 'dialog';
  labelledBy: string;
  className?: string;
  children: ReactNode;
}) {
  const ref = useSheetKeys(onClose);
  return (
    <div className="ck-scrim" onMouseDown={onClose}>
      <div
        ref={ref}
        className={`ck-sheet${className ? ` ${className}` : ''}`}
        role={role}
        aria-modal="true"
        aria-labelledby={labelledBy}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
