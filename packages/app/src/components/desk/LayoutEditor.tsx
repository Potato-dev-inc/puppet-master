// The layout: every computer's screens in one picture, to scale. Drag a computer to where it sits; it moves as one
// block and saves when you let go. Edges snap together unless you hold Shift (which flips the switch for that drag).
// The other computer is told and agrees. Keys: arrows nudge the focused computer, Shift for bigger steps.
import { useId, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import type { LayoutComputer, LayoutView } from '../../lib/desk-link';
import { collides, layoutPerPixel, nudged, snapPlace, viewBoxOf, type Rect } from '../../lib/desk-layout';

/** How close an edge must be, in canvas pixels, before it snaps. */
export const SNAP_PX = 10;

interface Drag {
  node: string;
  x: number;
  y: number;
  /** The picture is held still while a box is dragged. */
  view: Rect;
  /** On top of another computer's screens: it will not be saved. */
  blocked: boolean;
  saving: boolean;
}

interface Hold {
  node: string;
  startX: number;
  startY: number;
  from: { x: number; y: number };
  perPx: number;
  view: Rect;
  moved: boolean;
}

const pct = (v: number, origin: number, total: number): string => `${((v - origin) / total) * 100}%`;

function words(c: LayoutComputer): string {
  if (c.me) return 'This computer';
  if (!c.known) return 'Screen size unknown';
  return c.online ? 'Connected' : 'Offline';
}

export function LayoutEditor({
  layout,
  onPlace,
}: {
  layout: LayoutView | null;
  /** Saves a computer's place. Resolves false when it was refused. */
  onPlace: (node: string, x: number, y: number) => Promise<boolean>;
}) {
  const titleId = useId();
  const snapId = useId();
  const canvas = useRef<HTMLDivElement>(null);
  const hold = useRef<Hold | null>(null);
  const [snapOn, setSnapOn] = useState(true);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [said, setSaid] = useState('');

  if (!layout || layout.computers.length < 2) return null;
  const computers = layout.computers;
  const view = drag?.view ?? viewBoxOf(computers);

  const save = async (c: LayoutComputer, x: number, y: number, view: Rect) => {
    setDrag({ node: c.node, x, y, view, blocked: false, saving: true });
    const ok = await onPlace(c.node, x, y);
    setDrag(null);
    setSaid(ok ? `${c.name} placed at ${x}, ${y}.` : `${c.name} stayed where it was.`);
  };

  const onDown = (c: LayoutComputer, e: PointerEvent<HTMLElement>) => {
    if (e.button !== 0 || drag?.saving) return;
    const width = canvas.current?.getBoundingClientRect().width ?? 0;
    const v = viewBoxOf(computers);
    hold.current = { node: c.node, startX: e.clientX, startY: e.clientY, from: { x: c.x, y: c.y }, perPx: layoutPerPixel(v.width, width), view: v, moved: false };
    e.currentTarget.setPointerCapture?.(e.pointerId);
    setDrag({ node: c.node, x: c.x, y: c.y, view: v, blocked: false, saving: false });
  };

  const onMove = (c: LayoutComputer, e: PointerEvent<HTMLElement>) => {
    const h = hold.current;
    if (!h || h.node !== c.node) return;
    const raw = {
      x: Math.round(h.from.x + (e.clientX - h.startX) * h.perPx),
      y: Math.round(h.from.y + (e.clientY - h.startY) * h.perPx),
    };
    h.moved = h.moved || raw.x !== h.from.x || raw.y !== h.from.y;
    // Snapping is the switch, flipped while Shift is held.
    const snapping = snapOn !== e.shiftKey;
    const others = computers.filter((o) => o.node !== c.node);
    const at = snapping ? snapPlace(c, raw, others, SNAP_PX * h.perPx) : raw;
    setDrag({ node: c.node, x: at.x, y: at.y, view: h.view, blocked: collides(c, at.x, at.y, others), saving: false });
  };

  const onUp = (c: LayoutComputer) => {
    const h = hold.current;
    hold.current = null;
    if (!h || h.node !== c.node || !drag || drag.node !== c.node) return setDrag(null);
    if (!h.moved || drag.blocked || (drag.x === c.x && drag.y === c.y)) {
      if (drag.blocked) setSaid(`${c.name} cannot go on top of another screen.`);
      return setDrag(null);
    }
    void save(c, drag.x, drag.y, h.view);
  };

  const onCancel = () => {
    hold.current = null;
    setDrag((d) => (d?.saving ? d : null));
  };

  const onKey = (c: LayoutComputer, e: KeyboardEvent<HTMLElement>) => {
    if (e.key === 'Escape' && hold.current) {
      e.preventDefault();
      return onCancel();
    }
    const to = nudged(c, e.key, e.shiftKey);
    if (!to || drag?.saving) return;
    e.preventDefault();
    const others = computers.filter((o) => o.node !== c.node);
    if (collides(c, to.x, to.y, others)) return setSaid(`${c.name} cannot go on top of another screen.`);
    void save(c, to.x, to.y, viewBoxOf(computers));
  };

  return (
    <section className="ck-desk-section" aria-labelledby={titleId}>
      <div className="ck-desk-section-head">
        <h3 id={titleId} className="ck-desk-h3">
          Layout
        </h3>
        <div className="ck-desk-actions">
          <span id={snapId} className="ck-desk-row-label">
            Snap to edges
          </span>
          <button
            type="button"
            role="switch"
            aria-checked={snapOn}
            aria-labelledby={snapId}
            className={`ck-desk-switch${snapOn ? ' is-on' : ''}`}
            onClick={() => setSnapOn(!snapOn)}
          >
            <span className="ck-desk-switch-knob" aria-hidden="true" />
          </button>
        </div>
      </div>
      <div className="ck-desk-canvas ck-desk-lay" ref={canvas} style={{ aspectRatio: `${view.width} / ${view.height}` }} data-testid="desk-layout">
        {computers.map((c) => {
          const moving = drag?.node === c.node ? drag : null;
          const x = moving ? moving.x : c.x;
          const y = moving ? moving.y : c.y;
          const cls = [
            'ck-desk-lay-block',
            c.me ? 'is-me' : '',
            c.me || c.online ? '' : 'is-offline',
            c.known ? '' : 'is-unknown',
            moving ? 'is-dragging' : '',
            moving?.blocked ? 'is-blocked' : '',
          ]
            .filter(Boolean)
            .join(' ');
          const style = {
            left: pct(x, view.x, view.width),
            top: pct(y, view.y, view.height),
            width: `${(c.width / view.width) * 100}%`,
            height: `${(c.height / view.height) * 100}%`,
          };
          const inner = (
            <>
              {c.screens.map((s) => (
                <span
                  key={s.id}
                  className={`ck-desk-lay-screen${s.primary ? ' is-main' : ''}`}
                  aria-hidden="true"
                  style={{
                    left: pct(s.x, c.x, c.width),
                    top: pct(s.y, c.y, c.height),
                    width: `${(s.width / c.width) * 100}%`,
                    height: `${(s.height / c.height) * 100}%`,
                  }}
                >
                  <span className="ck-desk-screen-name">{`${s.width}×${s.height}`}</span>
                </span>
              ))}
              <span className="ck-desk-lay-chip">
                <span className="ck-desk-lay-name">{c.name}</span>
                <span className="ck-desk-lay-note">{moving ? `${x}, ${y}` : words(c)}</span>
              </span>
            </>
          );
          if (c.me) {
            return (
              <div key="me" className={cls} style={style} role="img" aria-label={`${c.name}, this computer, ${c.width} by ${c.height}`}>
                {inner}
              </div>
            );
          }
          return (
            <button
              key={c.node}
              type="button"
              className={cls}
              style={style}
              aria-label={`Move ${c.name}`}
              aria-describedby={`${titleId}-hint`}
              onPointerDown={(e) => onDown(c, e)}
              onPointerMove={(e) => onMove(c, e)}
              onPointerUp={() => onUp(c)}
              onPointerCancel={onCancel}
              onKeyDown={(e) => onKey(c, e)}
            >
              {inner}
            </button>
          );
        })}
      </div>
      <p id={`${titleId}-hint`} className="ck-desk-hint">
        Drag a computer to where it sits. Hold Shift to turn snapping {snapOn ? 'off' : 'on'} while you drag. Arrow keys move the focused computer.
      </p>
      <p className="ck-desk-hint" role="status">
        {said}
      </p>
    </section>
  );
}
