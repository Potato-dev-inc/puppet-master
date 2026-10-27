// The Summary | Normal | Verbose switch in the chat header. Small on the desktop. Under 600 px wide (a phone) it is
// one pill, "Normal ▾", that opens a small menu, so it sits on the same row as the model line (styles/cockpit-chat.css
// and styles/phone-chat.css give the phone a 44px touch area).
import { useEffect, useRef, useState } from 'react';
import { CHAT_VIEW_LABEL, CHAT_VIEW_MODES, type ChatViewMode } from './chatViewMode';

/** Narrower than this: the header shows the one-pill menu. */
export const COMPACT_VIEW_QUERY = '(max-width: 599px)';

/** True while the window is narrower than COMPACT_VIEW_QUERY. Follows resizes. */
export function useCompactViewMode(): boolean {
  const [compact, setCompact] = useState(() => typeof window !== 'undefined' && window.matchMedia?.(COMPACT_VIEW_QUERY).matches === true);
  useEffect(() => {
    const mq = typeof window !== 'undefined' ? window.matchMedia?.(COMPACT_VIEW_QUERY) : undefined;
    if (!mq) return;
    const onChange = (): void => setCompact(mq.matches);
    mq.addEventListener?.('change', onChange);
    return () => mq.removeEventListener?.('change', onChange);
  }, []);
  return compact;
}

export function ViewModeToggle({ mode, onChange }: { mode: ChatViewMode; onChange: (mode: ChatViewMode) => void }) {
  const compact = useCompactViewMode();
  if (compact) return <ViewModeMenu mode={mode} onChange={onChange} />;
  return (
    <div role="group" aria-label="Chat view" className="ml-auto flex shrink-0 items-center gap-0.5 rounded-md bg-pm-raised p-0.5 ck-chat-modes">
      {CHAT_VIEW_MODES.map((value) => (
        <button
          key={value}
          type="button"
          aria-pressed={mode === value}
          onClick={() => onChange(value)}
          className="rounded px-1.5 py-0.5 text-[11px] text-pm-muted ck-chat-mode"
        >
          {CHAT_VIEW_LABEL[value]}
        </button>
      ))}
    </div>
  );
}

/** The phone's compact switch: a pill with the current mode, and a small menu of the three modes under it. */
export function ViewModeMenu({ mode, onChange }: { mode: ChatViewMode; onChange: (mode: ChatViewMode) => void }) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);

  // A tap outside the pill and its menu, or Escape, closes the menu.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div ref={wrap} className="relative ml-auto shrink-0 ck-chat-mode-wrap">
      <button
        type="button"
        className="ck-chat-mode-pill"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`Chat view: ${CHAT_VIEW_LABEL[mode]}`}
        onClick={() => setOpen((v) => !v)}
      >
        {CHAT_VIEW_LABEL[mode]}
        <span aria-hidden="true"> ▾</span>
      </button>
      {open && (
        <div role="menu" aria-label="Chat view" className="ck-chat-mode-menu">
          {CHAT_VIEW_MODES.map((value) => (
            <button
              key={value}
              type="button"
              role="menuitemradio"
              aria-checked={mode === value}
              className="ck-chat-mode-item"
              onClick={() => {
                onChange(value);
                setOpen(false);
              }}
            >
              {CHAT_VIEW_LABEL[value]}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
