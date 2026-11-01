// The "Deleted. Undo" line after a delete (UX-2). It stays for 10 s, then calls `onExpire` and the parent removes it.
// Undo puts the item back from Recently deleted (the parent calls `trash.restore`).
import { useEffect, useRef } from 'react';
import { UNDO_MS } from '../../../lib/potato-trash';
import '../../../styles/cockpit-potato-trash.css';

export const UNDO_COPY = { undo: 'Undo' };

export interface UndoToastProps {
  message: string;
  onUndo: () => void;
  onExpire: () => void;
  durationMs?: number;
}

export function UndoToast({ message, onUndo, onExpire, durationMs = UNDO_MS }: UndoToastProps) {
  // The timer starts once per message; a new parent render must not restart it.
  const expire = useRef(onExpire);
  expire.current = onExpire;
  useEffect(() => {
    const timer = setTimeout(() => expire.current(), durationMs);
    return () => clearTimeout(timer);
  }, [message, durationMs]);

  return (
    <div role="status" className="ck-potato-undo">
      <span className="ck-potato-undo-text">{message}</span>
      <button type="button" className="ck-btn ck-btn--plain ck-btn--sm ck-potato-undo-btn" onClick={onUndo}>
        {UNDO_COPY.undo}
      </button>
    </div>
  );
}
