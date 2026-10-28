// The Focus pill under the sidebar header (SPEC n33 3.6 (2)). Only while Focus is on: "Focus · 31 min left" and an
// "End" button that ends it at once. The pill opens a small menu with "+15 min" and "End Focus". The time left is
// re-read every 30 seconds. Monochrome; the dot does not animate.
import { useEffect, useRef, useState } from 'react';
import { focusEnd, focusExtend, leftText } from '../../lib/focus';
import { focusIsOn } from './palette-focus';
import { useFocusView } from './useFocusView';
import '../../styles/focus-pill.css';

const TICK_MS = 30_000;

export function FocusPill({ notify }: { notify: (text: string) => void }) {
  const [view, setView] = useFocusView();
  const [now, setNow] = useState(() => Date.now());
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const on = focusIsOn(view, now);
  const endsAt = view?.endsAtMs ?? null;

  // The countdown: while Focus is on, the time left is re-read every 30 seconds.
  useEffect(() => {
    if (!on) return undefined;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), TICK_MS);
    return () => window.clearInterval(timer);
  }, [on, endsAt]);

  // The menu closes on a click outside it or on Escape.
  useEffect(() => {
    if (!open) return undefined;
    const onDown = (e: MouseEvent) => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  if (!on || endsAt === null) return null;

  const end = () => {
    setOpen(false);
    focusEnd()
      .then(setView)
      .catch(() => notify("Couldn't end Focus. Try again."));
  };
  const more = () => {
    setOpen(false);
    focusExtend()
      .then(setView)
      .catch(() => notify("Couldn't add time to Focus. Try again."));
  };

  return (
    <div className="ck-focus" ref={wrap}>
      <div className="ck-focus-pill" data-testid="focus-pill">
        <button
          type="button"
          className="ck-focus-main"
          aria-haspopup="menu"
          aria-expanded={open}
          title="Focus options"
          onClick={() => setOpen((v) => !v)}
        >
          <span className="ck-focus-dot" aria-hidden="true" />
          <span className="ck-focus-text">{`Focus · ${leftText(endsAt, now)}`}</span>
        </button>
        <button type="button" className="ck-focus-end" aria-label="End Focus" title="End Focus now" onClick={end}>
          End
        </button>
      </div>
      {open && (
        <div className="ck-menu ck-focus-menu" role="menu" aria-label="Focus">
          <button type="button" role="menuitem" className="ck-menu-item" onClick={more}>
            +15 min
          </button>
          <button type="button" role="menuitem" className="ck-menu-item" onClick={end}>
            End Focus
          </button>
        </div>
      )}
    </div>
  );
}
