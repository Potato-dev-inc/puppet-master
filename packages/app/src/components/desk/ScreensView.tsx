// The screens drawn to scale from their real rectangles. Click a screen to go there. Find my pointer draws a ring.
import { useEffect, useRef, useState } from 'react';
import type { DeskFind, DeskPointer, DeskScreen } from '../../lib/desk';

/** How long the find ring stays on screen. */
export const FIND_RING_MS = 1500;

interface Box {
  minX: number;
  minY: number;
  width: number;
  height: number;
}

/** The box around every screen, so each one can be drawn as a share of it. */
export function boundsOf(screens: DeskScreen[]): Box {
  const minX = Math.min(...screens.map((s) => s.x));
  const minY = Math.min(...screens.map((s) => s.y));
  const maxX = Math.max(...screens.map((s) => s.x + s.width));
  const maxY = Math.max(...screens.map((s) => s.y + s.height));
  return { minX, minY, width: Math.max(1, maxX - minX), height: Math.max(1, maxY - minY) };
}

/** A point or rectangle edge as a percentage of the box. */
function share(value: number, origin: number, total: number): string {
  return `${((value - origin) / total) * 100}%`;
}

export function ScreensView({
  screens,
  pointer,
  find,
  onHop,
  onFind,
  onRetry,
}: {
  screens: DeskScreen[];
  pointer: DeskPointer | null;
  find: DeskFind | null;
  onHop: (index: number) => void;
  onFind: () => void;
  onRetry: () => void;
}) {
  const seq = find?.seq;
  const seen = useRef(seq);
  const [ringOn, setRingOn] = useState(false);

  // A new "find my pointer" (a new seq) shows the ring. The first seq seen at mount does not.
  useEffect(() => {
    if (seq === undefined || seq === seen.current) return;
    seen.current = seq;
    setRingOn(true);
    const timer = setTimeout(() => setRingOn(false), FIND_RING_MS);
    return () => clearTimeout(timer);
  }, [seq]);

  const head = (
    <div className="ck-desk-section-head">
      <h3 className="ck-desk-h3">Screens</h3>
      <button type="button" className="ck-desk-btn" onClick={onFind} disabled={screens.length === 0}>
        Find my pointer
      </button>
    </div>
  );

  if (screens.length === 0) {
    return (
      <section className="ck-desk-section" aria-labelledby="ck-desk-screens-title">
        {head}
        <div className="ck-desk-empty">
          <p>No screens found. Plug in a screen, then try again.</p>
          <button type="button" className="ck-desk-btn" onClick={onRetry}>
            Try again
          </button>
        </div>
      </section>
    );
  }

  const box = boundsOf(screens);
  const ring = ringOn && find ? find : null;

  return (
    <section className="ck-desk-section" aria-labelledby="ck-desk-screens-title">
      {head}
      <div
        className="ck-desk-canvas"
        style={{ aspectRatio: `${box.width} / ${box.height}` }}
        data-testid="desk-canvas"
      >
        {screens.map((s) => (
          <button
            key={s.id}
            type="button"
            className={`ck-desk-screen${s.primary ? ' is-main' : ''}`}
            aria-label={`Go to screen ${s.index}`}
            style={{
              left: share(s.x, box.minX, box.width),
              top: share(s.y, box.minY, box.height),
              width: `${(s.width / box.width) * 100}%`,
              height: `${(s.height / box.height) * 100}%`,
            }}
            onClick={() => onHop(s.index)}
          >
            <span className="ck-desk-screen-name">
              {`Screen ${s.index} · ${s.width}×${s.height}${s.primary ? ' · main' : ''}`}
            </span>
          </button>
        ))}
        {pointer && (
          <span
            className="ck-desk-pointer"
            aria-hidden="true"
            style={{ left: share(pointer.x, box.minX, box.width), top: share(pointer.y, box.minY, box.height) }}
          />
        )}
        {ring && (
          <span
            className="ck-desk-ring"
            aria-hidden="true"
            style={{ left: share(ring.x, box.minX, box.width), top: share(ring.y, box.minY, box.height) }}
          />
        )}
      </div>
      <p className="ck-desk-hint" role="status">
        {ring ? ring.announce : ''}
      </p>
    </section>
  );
}
