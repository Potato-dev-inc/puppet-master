// @vitest-environment node
// The layout page's geometry: overlap, collision, snapping to edges, the picture's frame, keyboard nudges.
import { describe, expect, it } from 'vitest';
import { collides, layoutPerPixel, nudged, overlaps, screensAt, snapPlace, unionOf, viewBoxOf } from './desk-layout';
import type { LayoutComputer } from './desk-link';

/** Test-only fixture: a computer with one screen at (x, y). */
function box(node: string, x: number, y: number, width: number, height: number): LayoutComputer {
  return {
    node,
    name: node || 'Me',
    me: node === '',
    online: true,
    known: true,
    x,
    y,
    width,
    height,
    screens: [{ id: `${node}-1`, name: 's', x, y, width, height, primary: true }],
  };
}

const ME = box('', 0, 0, 1920, 1080);

describe('overlaps and collides', () => {
  it('touching edges are not an overlap, one pixel over is', () => {
    expect(overlaps({ x: 0, y: 0, width: 100, height: 100 }, { x: 100, y: 0, width: 100, height: 100 })).toBe(false);
    expect(overlaps({ x: 0, y: 0, width: 100, height: 100 }, { x: 99, y: 0, width: 100, height: 100 })).toBe(true);
  });

  it('checks every screen of the moving computer at its new place', () => {
    const two: LayoutComputer = {
      ...box('b', 2000, 0, 1000, 500),
      width: 1000,
      screens: [
        { id: '1', name: '1', x: 2000, y: 0, width: 500, height: 500, primary: true },
        { id: '2', name: '2', x: 2500, y: 0, width: 500, height: 500, primary: false },
      ],
    };
    expect(screensAt(two, 100, 50).map((r) => r.x)).toEqual([100, 600]);
    expect(collides(two, 1920, 0, [ME])).toBe(false);
    expect(collides(two, 1000, 0, [ME])).toBe(true);
    expect(collides(two, 1000, 2000, [ME])).toBe(false);
  });
});

describe('snapPlace', () => {
  const laptop = box('b', 2200, 100, 1280, 800);

  it('pulls a near edge to touch and keeps the other axis alone when nothing is near', () => {
    const at = snapPlace(laptop, { x: 1930, y: 400 }, [ME], 20);
    expect(at).toEqual({ x: 1920, y: 400 });
  });

  it('lines tops up when they are close', () => {
    const at = snapPlace(laptop, { x: 1925, y: 8 }, [ME], 20);
    expect(at).toEqual({ x: 1920, y: 0 });
  });

  it('snaps the bottom edges together as well', () => {
    const at = snapPlace(laptop, { x: 1925, y: 270 }, [ME], 20);
    expect(at).toEqual({ x: 1920, y: 280 });
  });

  it('pulls to the left of the big screen from the other side', () => {
    const at = snapPlace(laptop, { x: -1290, y: 200 }, [ME], 20);
    expect(at.x).toBe(-1280);
  });

  it('puts a computer below another one', () => {
    const at = snapPlace(laptop, { x: 15, y: 1090 }, [ME], 20);
    expect(at).toEqual({ x: 0, y: 1080 });
  });

  it('does nothing when no edge is within reach', () => {
    expect(snapPlace(laptop, { x: 3000, y: 2000 }, [ME], 20)).toEqual({ x: 3000, y: 2000 });
  });

  it('never snaps onto the other screens: it falls back to where the pointer is', () => {
    const wide = box('w', 5000, 0, 3000, 1080);
    // Snapping the left edge to ME's right edge would leave it touching; snapping inside would collide.
    const to = { x: 1910, y: 0 };
    const at = snapPlace(wide, to, [ME], 30);
    expect(collides(wide, at.x, at.y, [ME])).toBe(false);
  });

  it('prefers touching to aligning when both are as close', () => {
    // The left edge is 10 from ME's right edge (touch) and 10 from ME's left edge... only touch is reachable here.
    const at = snapPlace(box('c', 3000, 0, 500, 500), { x: 1910, y: 2000 }, [ME], 20);
    expect(at.x).toBe(1920);
  });

  it('returns the place unchanged with nothing to snap to', () => {
    expect(snapPlace(laptop, { x: 5, y: 6 }, [], 20)).toEqual({ x: 5, y: 6 });
  });
});

describe('the picture', () => {
  it('has room around every computer, including negative places', () => {
    const left = box('b', -1300, -100, 1280, 800);
    const v = viewBoxOf([ME, left]);
    expect(v.x).toBeLessThan(-1300);
    expect(v.y).toBeLessThan(-100);
    expect(v.x + v.width).toBeGreaterThan(1920);
    expect(v.y + v.height).toBeGreaterThan(1080);
  });

  it('is a standard frame with nothing in it', () => {
    expect(viewBoxOf([])).toEqual({ x: 0, y: 0, width: 1920, height: 1080 });
    expect(unionOf([])).toBeNull();
  });

  it('turns canvas pixels into layout pixels', () => {
    expect(layoutPerPixel(4000, 400)).toBe(10);
    expect(layoutPerPixel(4000, 0)).toBe(1);
  });
});

describe('nudged', () => {
  it('moves by 20, or 200 with Shift, and ignores other keys', () => {
    const c = box('b', 100, 100, 10, 10);
    expect(nudged(c, 'ArrowRight', false)).toEqual({ x: 120, y: 100 });
    expect(nudged(c, 'ArrowUp', true)).toEqual({ x: 100, y: -100 });
    expect(nudged(c, 'ArrowLeft', false)).toEqual({ x: 80, y: 100 });
    expect(nudged(c, 'ArrowDown', false)).toEqual({ x: 100, y: 120 });
    expect(nudged(c, 'a', false)).toBeNull();
  });
});
