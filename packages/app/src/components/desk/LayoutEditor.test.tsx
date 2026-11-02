// @vitest-environment jsdom
// The layout: computers drawn to scale in one picture, dragged as blocks, snapped, saved on drop, refused when they overlap.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LayoutEditor } from './LayoutEditor';
import type { LayoutComputer, LayoutView } from '../../lib/desk-link';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** Test-only fixtures. */
const ME: LayoutComputer = {
  node: '',
  name: 'Ren-PC',
  me: true,
  online: true,
  known: true,
  x: 0,
  y: 0,
  width: 1920,
  height: 1080,
  screens: [{ id: 'm', name: 'm', x: 0, y: 0, width: 1920, height: 1080, primary: true }],
};
const LAPTOP: LayoutComputer = {
  node: 'ab'.repeat(32),
  name: 'Laptop',
  me: false,
  online: true,
  known: true,
  x: 1920,
  y: 0,
  width: 1280,
  height: 800,
  screens: [{ id: 'l', name: 'l', x: 1920, y: 0, width: 1280, height: 800, primary: true }],
};
const view = (...computers: LayoutComputer[]): LayoutView => ({ computers, touch_gap: 80 });

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function render(layout: LayoutView | null, onPlace = vi.fn(async () => true)) {
  act(() => root.render(<LayoutEditor layout={layout} onPlace={onPlace} />));
  return onPlace;
}

const block = (name: string) => host.querySelector<HTMLButtonElement>(`[aria-label="Move ${name}"]`)!;
const canvas = () => host.querySelector<HTMLElement>('[data-testid="desk-layout"]')!;

/** The canvas is 400 px wide in the test, so the picture (6400 wide with its margin) is 16 layout px per pixel. */
function sizeCanvas(width = 400) {
  canvas().getBoundingClientRect = () => ({ width, height: 200, left: 0, top: 0, right: width, bottom: 200, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
}

function pointer(el: Element, type: string, x: number, y: number, shiftKey = false) {
  act(() => {
    el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, shiftKey }));
  });
}

const flush = () => act(async () => {});

describe('LayoutEditor', () => {
  it('draws nothing until a second computer is paired', () => {
    render(null);
    expect(host.textContent).toBe('');
    render(view(ME));
    expect(host.textContent).toBe('');
  });

  it('draws every computer to scale in one picture, with plain words for each', () => {
    render(view(ME, LAPTOP));
    expect(host.textContent).toContain('Layout');
    expect(host.textContent).toContain('This computer');
    expect(host.textContent).toContain('Connected');
    expect(host.textContent).toContain('1920×1080');
    expect(host.textContent).toContain('1280×800');
    const mine = host.querySelector<HTMLElement>('[role="img"]')!;
    const theirs = block('Laptop');
    expect(parseFloat(mine.style.width)).toBeGreaterThan(parseFloat(theirs.style.width));
    expect(parseFloat(theirs.style.left)).toBeGreaterThan(parseFloat(mine.style.left));
  });

  it('says offline and unknown in words', () => {
    render(view(ME, { ...LAPTOP, online: false }));
    expect(host.textContent).toContain('Offline');
    render(view(ME, { ...LAPTOP, known: false }));
    expect(host.textContent).toContain('Screen size unknown');
  });

  it('only the other computers can be moved', () => {
    render(view(ME, LAPTOP));
    expect(host.querySelectorAll('button[aria-label^="Move "]').length).toBe(1);
  });

  it('dragging a computer moves the whole block and saves where it was dropped', async () => {
    const onPlace = render(view(ME, LAPTOP));
    sizeCanvas();
    // Snapping off for a clean number: 40 px right and 20 px down on the canvas is 640 and 320 layout px.
    act(() => host.querySelector<HTMLButtonElement>('[role="switch"]')!.click());
    const el = block('Laptop');
    pointer(el, 'pointerdown', 100, 100);
    pointer(el, 'pointermove', 140, 120);
    expect(block('Laptop').textContent).toContain('2560, 320');
    pointer(block('Laptop'), 'pointerup', 140, 120);
    await flush();
    expect(onPlace).toHaveBeenCalledTimes(1);
    expect(onPlace).toHaveBeenCalledWith(LAPTOP.node, 2560, 320);
    expect(host.querySelector('[role="status"]')!.textContent).toBe('Laptop placed at 2560, 320.');
  });

  it('snaps its edge to the other computer, and Shift flips it for one drag', async () => {
    const onPlace = render(view(ME, LAPTOP));
    sizeCanvas();
    const el = block('Laptop');
    // 3 px of canvas to the right of where it touches is within the 10 px snap: it lands touching again.
    pointer(el, 'pointerdown', 100, 100);
    pointer(el, 'pointermove', 103, 100);
    pointer(block('Laptop'), 'pointerup', 103, 100);
    await flush();
    expect(onPlace).not.toHaveBeenCalled();
    // The same move with Shift held is not snapped, so it is saved where it is.
    pointer(el, 'pointerdown', 100, 100);
    pointer(el, 'pointermove', 103, 100, true);
    pointer(block('Laptop'), 'pointerup', 103, 100, true);
    await flush();
    expect(onPlace).toHaveBeenCalledWith(LAPTOP.node, 1968, 0);
  });

  it('with snapping off in the switch, Shift turns it on for that drag', async () => {
    const onPlace = render(view(ME, LAPTOP));
    sizeCanvas();
    act(() => host.querySelector<HTMLButtonElement>('[role="switch"]')!.click());
    expect(host.querySelector('[role="switch"]')!.getAttribute('aria-checked')).toBe('false');
    expect(host.textContent).toContain('Hold Shift to turn snapping on');
    const el = block('Laptop');
    pointer(el, 'pointerdown', 100, 100);
    pointer(el, 'pointermove', 103, 100, true);
    pointer(block('Laptop'), 'pointerup', 103, 100, true);
    await flush();
    expect(onPlace).not.toHaveBeenCalled();
  });

  it('a drop on top of this computer is not saved and says why', async () => {
    const onPlace = render(view(ME, LAPTOP));
    sizeCanvas();
    act(() => host.querySelector<HTMLButtonElement>('[role="switch"]')!.click());
    const el = block('Laptop');
    pointer(el, 'pointerdown', 100, 100);
    pointer(el, 'pointermove', 40, 100);
    expect(block('Laptop').className).toContain('is-blocked');
    pointer(block('Laptop'), 'pointerup', 40, 100);
    await flush();
    expect(onPlace).not.toHaveBeenCalled();
    expect(host.querySelector('[role="status"]')!.textContent).toContain('cannot go on top of another screen');
    expect(block('Laptop').className).not.toContain('is-dragging');
  });

  it('a click without moving saves nothing', async () => {
    const onPlace = render(view(ME, LAPTOP));
    sizeCanvas();
    const el = block('Laptop');
    pointer(el, 'pointerdown', 100, 100);
    pointer(block('Laptop'), 'pointerup', 100, 100);
    await flush();
    expect(onPlace).not.toHaveBeenCalled();
  });

  it('a refused save puts the block back and says it stayed', async () => {
    const onPlace = vi.fn(async () => false);
    render(view(ME, LAPTOP), onPlace);
    sizeCanvas();
    act(() => host.querySelector<HTMLButtonElement>('[role="switch"]')!.click());
    const el = block('Laptop');
    pointer(el, 'pointerdown', 100, 100);
    pointer(el, 'pointermove', 100, 140);
    pointer(block('Laptop'), 'pointerup', 100, 140);
    await flush();
    expect(onPlace).toHaveBeenCalled();
    expect(host.querySelector('[role="status"]')!.textContent).toBe('Laptop stayed where it was.');
    expect(block('Laptop').className).not.toContain('is-dragging');
  });

  it('arrow keys nudge the focused computer, Shift for bigger steps, and refuse to overlap', async () => {
    const onPlace = render(view(ME, LAPTOP));
    const el = block('Laptop');
    act(() => {
      el.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    });
    await flush();
    expect(onPlace).toHaveBeenLastCalledWith(LAPTOP.node, 1920, 20);
    act(() => {
      el.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, bubbles: true, cancelable: true }));
    });
    await flush();
    expect(onPlace).toHaveBeenLastCalledWith(LAPTOP.node, 2120, 0);
    act(() => {
      el.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true }));
    });
    await flush();
    expect(onPlace).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[role="status"]')!.textContent).toContain('cannot go on top');
  });
});
