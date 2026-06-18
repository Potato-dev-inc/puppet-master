import { describe, expect, it } from 'vitest';
import { MobileTouchScroll } from './mobile-touch-scroll';

function dispatchPointer(
  target: EventTarget,
  type: string,
  init: { clientY: number; pointerId: number },
): void {
  const event = new Event(type, { bubbles: true, cancelable: true }) as PointerEvent;
  Object.defineProperties(event, {
    clientY: { value: init.clientY },
    pointerId: { value: init.pointerId },
    pointerType: { value: 'touch' },
    button: { value: 0 },
  });
  target.dispatchEvent(event);
}

describe('MobileTouchScroll', () => {
  function createScroll(lineHeightPx = 20) {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const scrolled: number[] = [];
    const taps: EventTarget[] = [];
    const ignore = document.createElement('input');
    ignore.className = 'terminal-mobile-command-input';
    container.appendChild(ignore);

    const touch = new MobileTouchScroll({
      container,
      lineHeightPx,
      scrollLines: (lines) => scrolled.push(lines),
      shouldIgnoreTarget: (target) =>
        target instanceof Element && target.closest('.terminal-mobile-command-input') != null,
      onBackgroundTap: (target) => {
        if (target) taps.push(target);
      },
    });

    return {
      cleanup: () => {
        touch.dispose();
        container.remove();
      },
      container,
      ignore,
      scrolled,
      taps,
    };
  }

  function drag(container: HTMLElement, fromY: number, toY: number): void {
    dispatchPointer(container, 'pointerdown', { clientY: fromY, pointerId: 1 });
    dispatchPointer(container, 'pointermove', { clientY: toY, pointerId: 1 });
    dispatchPointer(container, 'pointerup', { clientY: toY, pointerId: 1 });
  }

  it('scrolls down when the finger moves down', () => {
    const { cleanup, container, scrolled } = createScroll();
    drag(container, 100, 140);
    expect(scrolled).toEqual([2]);
    cleanup();
  });

  it('scrolls up when the finger moves up', () => {
    const { cleanup, container, scrolled } = createScroll();
    drag(container, 140, 100);
    expect(scrolled).toEqual([-2]);
    cleanup();
  });

  it('fires onBackgroundTap for a short tap without scrolling', () => {
    const { cleanup, container, scrolled, taps } = createScroll();
    const background = document.createElement('div');
    container.appendChild(background);

    dispatchPointer(background, 'pointerdown', { clientY: 50, pointerId: 2 });
    dispatchPointer(background, 'pointerup', { clientY: 52, pointerId: 2 });

    expect(scrolled).toEqual([]);
    expect(taps).toEqual([background]);
    cleanup();
  });

  it('ignores drags that start on the command input', () => {
    const { cleanup, ignore, scrolled } = createScroll();

    dispatchPointer(ignore, 'pointerdown', { clientY: 10, pointerId: 3 });
    dispatchPointer(ignore, 'pointermove', { clientY: 80, pointerId: 3 });
    dispatchPointer(ignore, 'pointerup', { clientY: 80, pointerId: 3 });

    expect(scrolled).toEqual([]);
    cleanup();
  });
});
