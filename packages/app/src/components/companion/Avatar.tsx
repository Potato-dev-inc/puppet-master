// The companion's face (SPEC-1028 section 10): one of the eight dot bitmaps in avatars.ts, with the overlay and
// the motion of its state. Monochrome: every dot is currentColor, so the face follows the text colour in light and
// dark mode. Under prefers-reduced-motion no motion class is rendered, and the blink timer does not run.
import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import type { AvatarId, CompanionState } from '../../lib/companion';
import { stateWord } from '../../lib/companion';
import { AVATARS, GRID, parseBitmap } from './avatars';
import '../../styles/companion-avatar.css';

/** Below this many px: dim dots become solid at half opacity, and only the `!` overlay is drawn. */
export const SMALL_PX = 28;
/** Idle blink: the eyes close for BLINK_MS, every BLINK_MIN_MS to BLINK_MAX_MS. */
export const BLINK_MIN_MS = 4000;
export const BLINK_MAX_MS = 7000;
export const BLINK_MS = 120;

/** The view box is 15 cells wide: the creature in columns 0 to 11, the overlay in columns 12 to 14. */
const VIEW_W = 15;
const DOT_R = 0.42;
const SMALL_DIM_OPACITY = 0.5;
const RIPPLE_STEP_MS = 300;
const ROW_STEP_MS = 40;

/** Overlay cells as [column, row] (SPEC section 10). */
const WORKING_CELLS: [number, number][] = [
  [12, 3],
  [13, 3],
  [14, 3],
];
const BANG_CELLS: [number, number][] = [
  [13, 0],
  [13, 1],
  [13, 2],
  [13, 4],
];
const Z_CELLS: [number, number][] = [
  [12, 0],
  [13, 0],
  [14, 0],
  [13, 1],
  [12, 2],
  [13, 2],
  [14, 2],
];

const REDUCED_QUERY = '(prefers-reduced-motion: reduce)';

/** True while the person asks for less motion. Follows changes while the face is on screen. */
export function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() => typeof window !== 'undefined' && window.matchMedia?.(REDUCED_QUERY).matches === true);
  useEffect(() => {
    const mq = typeof window !== 'undefined' ? window.matchMedia?.(REDUCED_QUERY) : undefined;
    if (!mq) return;
    const onChange = (): void => setReduced(mq.matches);
    mq.addEventListener?.('change', onChange);
    return () => mq.removeEventListener?.('change', onChange);
  }, []);
  return reduced;
}

/** Whether the eyes are closed for a blink right now. Runs only while `active`: a random 4 to 7 s wait, then 120 ms. */
function useBlink(active: boolean): boolean {
  const [blink, setBlink] = useState(false);
  useEffect(() => {
    if (!active) return;
    let openTimer: ReturnType<typeof setTimeout> | undefined;
    let closeTimer: ReturnType<typeof setTimeout> | undefined;
    const schedule = (): void => {
      const wait = BLINK_MIN_MS + Math.random() * (BLINK_MAX_MS - BLINK_MIN_MS);
      openTimer = setTimeout(() => {
        setBlink(true);
        closeTimer = setTimeout(() => {
          setBlink(false);
          schedule();
        }, BLINK_MS);
      }, wait);
    };
    schedule();
    return () => {
      clearTimeout(openTimer);
      clearTimeout(closeTimer);
    };
  }, [active]);
  return blink;
}

export interface AvatarProps {
  id: AvatarId;
  /** Width in px. The face is drawn 15 by 12 cells, so the height is 0.8 of this. */
  size: number;
  state: CompanionState;
  /** The companion's own name for the label. Defaults to the face's name. */
  name?: string;
  className?: string;
}

const cell = (c: number, r: number): { cx: number; cy: number } => ({ cx: c + 0.5, cy: r + 0.5 });

export function Avatar({ id, size, state, name, className }: AvatarProps): JSX.Element {
  const def = AVATARS[id];
  const reduced = usePrefersReducedMotion();
  const blinking = useBlink(state === 'idle' && !reduced);
  const cells = useMemo(() => parseBitmap(def.rows), [def]);
  const small = size < SMALL_PX;
  const eyesClosed = state === 'sleeping' || state === 'stopped' || blinking;
  const move = (cls: string): string | undefined => (reduced ? undefined : cls);

  const body: JSX.Element[] = [];
  cells.forEach((row, r) => {
    row.forEach((kind, c) => {
      if (kind === 'empty') return;
      // `+` cells and closed eyes are dim dots; below SMALL_PX they become solid dots at half opacity.
      const dimmed = kind === 'dim' || (kind === 'eye' && eyesClosed);
      if (kind === 'eye' && !eyesClosed && def.eyes === 'hole') return;
      const classes = ['ca-dot'];
      let opacity: number | undefined;
      if (dimmed) {
        if (small) opacity = SMALL_DIM_OPACITY;
        else classes.push('ca-dim');
      }
      const motion = state === 'starting' ? move('ca-move-row') : undefined;
      if (motion) classes.push(motion);
      const style: CSSProperties | undefined = motion ? { animationDelay: `${(GRID - 1 - r) * ROW_STEP_MS}ms` } : undefined;
      body.push(<circle key={`${r}-${c}`} {...cell(c, r)} r={DOT_R} className={classes.join(' ')} opacity={opacity} style={style} />);
    });
  });

  const overlay: JSX.Element[] = [];
  const dot = (key: string, c: number, r: number, cls: string, style?: CSSProperties): JSX.Element => (
    <circle key={key} {...cell(c, r)} r={DOT_R} className={cls} style={style} />
  );
  if (state === 'working' && !small) {
    WORKING_CELLS.forEach(([c, r], i) => {
      const motion = move('ca-move-ripple');
      overlay.push(dot(`w${i}`, c, r, ['ca-ripple', motion].filter(Boolean).join(' '), motion ? { animationDelay: `${-i * RIPPLE_STEP_MS}ms` } : undefined));
    });
  }
  if (state === 'needs_you') {
    BANG_CELLS.forEach(([c, r], i) => overlay.push(dot(`b${i}`, c, r, 'ca-dot')));
  }
  if (state === 'sleeping' && !small) {
    Z_CELLS.forEach(([c, r], i) => {
      const motion = move('ca-move-z');
      overlay.push(dot(`z${i}`, c, r, ['ca-z', motion].filter(Boolean).join(' ')));
    });
  }

  const rootClass = ['ca-avatar', state === 'stopped' ? 'ca-stopped' : '', className ?? ''].filter(Boolean).join(' ');
  const bodyClass = [state === 'sleeping' ? 'ca-body-sleep' : '', state === 'needs_you' ? move('ca-move-hop') ?? '' : ''].filter(Boolean).join(' ');

  return (
    <svg
      role="img"
      aria-label={`${name ?? def.name}, ${stateWord(state)}`}
      focusable="false"
      className={rootClass}
      data-state={state}
      width={size}
      height={(size * GRID) / VIEW_W}
      viewBox={`0 0 ${VIEW_W} ${GRID}`}
    >
      <g className={bodyClass || undefined}>{body}</g>
      {overlay.length > 0 ? <g>{overlay}</g> : null}
    </svg>
  );
}
