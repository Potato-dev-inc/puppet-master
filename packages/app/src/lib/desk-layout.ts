// The layout page's geometry: pure functions, so the drag and the snap are tested without a browser.
// Everything is in layout pixels (the shared space, where this computer's box starts at 0, 0).
import type { LayoutComputer, LayoutScreen } from './desk-link';

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Point {
  x: number;
  y: number;
}

const rectOf = (s: LayoutScreen): Rect => ({ x: s.x, y: s.y, width: s.width, height: s.height });

/** The box around a computer's screens. */
export function boxOf(c: LayoutComputer): Rect {
  return { x: c.x, y: c.y, width: c.width, height: c.height };
}

/** The box around every rectangle, or null when there are none. */
export function unionOf(rects: Rect[]): Rect | null {
  if (rects.length === 0) return null;
  const left = Math.min(...rects.map((r) => r.x));
  const top = Math.min(...rects.map((r) => r.y));
  const right = Math.max(...rects.map((r) => r.x + r.width));
  const bottom = Math.max(...rects.map((r) => r.y + r.height));
  return { x: left, y: top, width: Math.max(1, right - left), height: Math.max(1, bottom - top) };
}

/** Whether two rectangles share area. Touching edges do not count. */
export function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

/** A computer's screens moved so its box starts at (x, y). */
export function screensAt(c: LayoutComputer, x: number, y: number): Rect[] {
  return c.screens.map((s) => ({ ...rectOf(s), x: s.x - c.x + x, y: s.y - c.y + y }));
}

/** Whether moving `moving` to (x, y) puts any of its screens on top of a screen of another computer. */
export function collides(moving: LayoutComputer, x: number, y: number, others: LayoutComputer[]): boolean {
  const mine = screensAt(moving, x, y);
  return others.some((o) => o.screens.some((s) => mine.some((m) => overlaps(m, rectOf(s)))));
}

/** The smallest move (within `limit`) that lines one of `edges` up with one of `targets`, or null. */
function pull(edges: number[], targets: number[], limit: number): number | null {
  let best: number | null = null;
  for (const e of edges) {
    for (const t of targets) {
      const d = t - e;
      if (Math.abs(d) <= limit && (best === null || Math.abs(d) < Math.abs(best))) best = d;
    }
  }
  return best;
}

/** Touching wins over aligning when they are as close as each other. */
function prefer(touch: number | null, align: number | null): number {
  if (touch !== null && (align === null || Math.abs(touch) <= Math.abs(align))) return touch;
  return align ?? 0;
}

/**
 * Pulls a computer being dragged to the edges of the other computers' screens: its left or right edge to their
 * right or left edge (touching) or to the same edge (aligned), and the same for top and bottom. Each axis moves
 * on its own. `limit` is how close counts, in layout pixels. Falls back to the unsnapped place when snapping
 * would put the screens on top of each other.
 */
export function snapPlace(moving: LayoutComputer, to: Point, others: LayoutComputer[], limit: number): Point {
  const mine = screensAt(moving, to.x, to.y);
  const theirs = others.flatMap((o) => o.screens.map(rectOf));
  if (mine.length === 0 || theirs.length === 0) return to;
  const lefts = mine.map((r) => r.x);
  const rights = mine.map((r) => r.x + r.width);
  const tops = mine.map((r) => r.y);
  const bottoms = mine.map((r) => r.y + r.height);
  const theirLefts = theirs.map((r) => r.x);
  const theirRights = theirs.map((r) => r.x + r.width);
  const theirTops = theirs.map((r) => r.y);
  const theirBottoms = theirs.map((r) => r.y + r.height);
  const dx = prefer(
    closest(pull(lefts, theirRights, limit), pull(rights, theirLefts, limit)),
    closest(pull(lefts, theirLefts, limit), pull(rights, theirRights, limit)),
  );
  const dy = prefer(
    closest(pull(tops, theirBottoms, limit), pull(bottoms, theirTops, limit)),
    closest(pull(tops, theirTops, limit), pull(bottoms, theirBottoms, limit)),
  );
  const snapped = { x: Math.round(to.x + dx), y: Math.round(to.y + dy) };
  return collides(moving, snapped.x, snapped.y, others) ? to : snapped;
}

function closest(a: number | null, b: number | null): number | null {
  if (a === null) return b;
  if (b === null) return a;
  return Math.abs(a) <= Math.abs(b) ? a : b;
}

/** Pixels in the layout per pixel on the canvas (so a drag of 10 canvas pixels moves the box by 10 times this). */
export function layoutPerPixel(boundsWidth: number, canvasWidth: number): number {
  return canvasWidth > 0 ? boundsWidth / canvasWidth : 1;
}

/**
 * The area the canvas shows: every computer with room around it, so a dragged box has somewhere to go.
 * The room is half of the longer side on each side, which keeps the picture from jumping while the box moves
 * (the page holds this box fixed for the length of a drag).
 */
export function viewBoxOf(computers: LayoutComputer[]): Rect {
  const all = unionOf(computers.flatMap((c) => c.screens.map(rectOf)));
  if (!all) return { x: 0, y: 0, width: 1920, height: 1080 };
  const room = Math.round(Math.max(all.width, all.height) / 2);
  return { x: all.x - room, y: all.y - room, width: all.width + room * 2, height: all.height + room * 2 };
}

/** The place a keyboard nudge asks for. */
export function nudged(c: LayoutComputer, key: string, big: boolean): Point | null {
  const step = big ? 200 : 20;
  switch (key) {
    case 'ArrowLeft':
      return { x: c.x - step, y: c.y };
    case 'ArrowRight':
      return { x: c.x + step, y: c.y };
    case 'ArrowUp':
      return { x: c.x, y: c.y - step };
    case 'ArrowDown':
      return { x: c.x, y: c.y + step };
    default:
      return null;
  }
}
