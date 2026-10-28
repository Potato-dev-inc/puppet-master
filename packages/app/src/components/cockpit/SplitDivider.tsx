// The divider between the two split sides. Drag it to resize (each side keeps its minimum width), double-click
// it to go back to an even split, or focus it and press Left or Right to move it one step.
import { useRef, type KeyboardEvent, type PointerEvent, type RefObject } from 'react';
import { SPLIT_DEFAULT_RATIO, SPLIT_DIVIDER_PX, SPLIT_KEY_STEP_PX, nudgeSplitRatio, ratioFromLeftPx } from './main-view';

export function SplitDivider({
  colsRef,
  ratio,
  onRatio,
}: {
  /** The element both sides sit in. Its width is the room the two sides share (with the divider). */
  colsRef: RefObject<HTMLDivElement | null>;
  ratio: number;
  onRatio: (ratio: number) => void;
}) {
  // True while the pointer is held down on the divider (a drag). A ref: each move re-renders this component.
  const dragging = useRef(false);
  const span = () => (colsRef.current ? colsRef.current.getBoundingClientRect().width - SPLIT_DIVIDER_PX : 0);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    onRatio(nudgeSplitRatio(ratio, e.key === 'ArrowLeft' ? -SPLIT_KEY_STEP_PX : SPLIT_KEY_STEP_PX, span()));
  };
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    dragging.current = true;
    // Keeps the moves coming while the pointer leaves the thin divider (not in every test DOM, hence the ?.).
    e.currentTarget.setPointerCapture?.(e.pointerId);
    // A drag selects no text. The divider takes the focus itself, so the arrow keys work right after a drag.
    e.preventDefault();
    e.currentTarget.focus({ preventScroll: true });
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const cols = colsRef.current;
    if (!dragging.current || !cols) return;
    const rect = cols.getBoundingClientRect();
    onRatio(ratioFromLeftPx(e.clientX - rect.left - SPLIT_DIVIDER_PX / 2, rect.width - SPLIT_DIVIDER_PX));
  };
  const endDrag = (e: PointerEvent<HTMLDivElement>) => {
    dragging.current = false;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
  };

  return (
    <div
      className="ck-split-divider"
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-label="Resize the two chats"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(ratio * 100)}
      title="Drag to resize. Double-click to split evenly. Left and Right arrows move it."
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onDoubleClick={() => onRatio(SPLIT_DEFAULT_RATIO)}
    />
  );
}
