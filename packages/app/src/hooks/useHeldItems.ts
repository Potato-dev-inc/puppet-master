// A new card never moves under a thumb: while the person is touching the list (or lifted their finger
// less than 600 ms ago), an item that was not on screen before waits. Items already drawn keep updating
// in place, so a card that gets answered still shows its receipt at once.
import { useEffect, useRef, useState } from 'react';
import type { AttentionItem } from '../lib/attention';
import { TOUCH_HOLD_MS, withoutNewItems } from '../lib/needs-you-queue';

let down = false;
let lastTouchAt = 0;
let listening = false;

function listen() {
  if (listening || typeof window === 'undefined') return;
  listening = true;
  const mark = () => {
    lastTouchAt = Date.now();
  };
  window.addEventListener('pointerdown', () => {
    down = true;
    mark();
  }, true);
  const up = () => {
    down = false;
    mark();
  };
  window.addEventListener('pointerup', up, true);
  window.addEventListener('pointercancel', up, true);
}

/** Milliseconds until a finger-free list may take a new card; 0 = now. */
export function touchHoldLeft(now = Date.now()): number {
  if (down) return TOUCH_HOLD_MS;
  return Math.max(0, lastTouchAt + TOUCH_HOLD_MS - now);
}

/** Test hook: forget every touch. */
export function resetTouchForTests(): void {
  down = false;
  lastTouchAt = 0;
}

export function useHeldItems(items: AttentionItem[]): AttentionItem[] {
  listen();
  const known = useRef<Set<string>>(new Set(items.map((i) => i.id)));
  const [, release] = useState(0);
  const left = touchHoldLeft();
  const holding = left > 0 && items.some((i) => !known.current.has(i.id));
  const shown = holding ? withoutNewItems(items, known.current) : items;
  if (!holding) known.current = new Set(items.map((i) => i.id));

  useEffect(() => {
    if (!holding) return;
    const timer = window.setTimeout(() => release((n) => n + 1), Math.max(touchHoldLeft(), 16) + 10);
    return () => window.clearTimeout(timer);
  });
  return shown;
}
