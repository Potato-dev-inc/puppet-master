// The thin draggable edge between two regions of the Potato Bot page. Drag it, or focus it and press Left / Right
// (32 px a press) or Home / End (the smallest and the largest). Double-click goes back to the default width.
// `grows` says which way the region gets bigger: the bot list grows to the right, the panel grows to the left.
import { useRef, type KeyboardEvent, type PointerEvent } from 'react';
import { KEY_STEP } from '../../../lib/potato-layout';

export interface ResizeHandleProps {
  label: string;
  grows: 'right' | 'left';
  /** The region's width now, in px (what is shown, so a collapsed list reports its strip). */
  value: number;
  min: number;
  max: number;
  /** The width the person asks for, before any limit. The caller clamps it. */
  onChange: (raw: number, via: 'drag' | 'key') => void;
  onReset?: () => void;
}

export function ResizeHandle({ label, grows, value, min, max, onChange, onReset }: ResizeHandleProps) {
  // The pointer's start and the width then; a ref, because every move re-renders the page.
  const drag = useRef<{ x: number; width: number } | null>(null);
  const sign = grows === 'right' ? 1 : -1;

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Home') {
      e.preventDefault();
      onChange(min, 'key');
    } else if (e.key === 'End') {
      e.preventDefault();
      onChange(max, 'key');
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      onChange(value + (e.key === 'ArrowRight' ? 1 : -1) * sign * KEY_STEP, 'key');
    }
  };
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    drag.current = { x: e.clientX, width: value };
    // Keeps the moves coming while the pointer leaves the thin edge (not in every test DOM, hence the ?.).
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.preventDefault();
    e.currentTarget.focus({ preventScroll: true });
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (start) onChange(start.width + sign * (e.clientX - start.x), 'drag');
  };
  const end = (e: PointerEvent<HTMLDivElement>) => {
    drag.current = null;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
  };

  return (
    <div
      className="ck-potato-resize"
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(value)}
      title="Drag to resize. Double-click to reset."
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={end}
      onPointerCancel={end}
      onDoubleClick={onReset}
    />
  );
}
