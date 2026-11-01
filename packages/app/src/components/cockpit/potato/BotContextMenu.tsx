// The menu of one bot: opened by the row's "..." button or a right-click. Arrow keys move between the items, Escape or a
// click outside closes it, and it stays on the screen. It shows what lib/potato-list.ts (botMenuItems) gives it.
import { useEffect, useRef, type KeyboardEvent } from 'react';
import { LIST_COPY, type BotMenuAction, type BotMenuItem } from '../../../lib/potato-list';
import '../../../styles/cockpit-potato-list.css';

export interface BotContextMenuProps {
  x: number;
  y: number;
  items: BotMenuItem[];
  onPick: (action: BotMenuAction) => void;
  onClose: () => void;
}

const WIDTH = 168;
const ITEM_HEIGHT = 30;
const EDGE = 8;

export function BotContextMenu({ x, y, items, onPick, onClose }: BotContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('[role="menuitem"]:not([disabled])')?.focus();
  }, []);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    document.addEventListener('mousedown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('keydown', onKey, true);
    };
  }, [onClose]);

  const left = Math.max(EDGE, Math.min(x, window.innerWidth - WIDTH - EDGE));
  const top = Math.max(EDGE, Math.min(y, window.innerHeight - items.length * ITEM_HEIGHT - EDGE * 2));

  const onMenuKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not([disabled])') ?? [])];
    if (buttons.length === 0) return;
    e.preventDefault();
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.key === 'ArrowDown' ? (at + 1) % buttons.length : (at - 1 + buttons.length) % buttons.length;
    buttons[next].focus();
  };

  return (
    <div ref={ref} className="ck-potato-menu" role="menu" aria-label={LIST_COPY.menu} style={{ left, top }} onKeyDown={onMenuKey}>
      {items.map((item) => (
        <button key={item.action} type="button" role="menuitem" className="ck-potato-menu-item" disabled={item.disabled} onClick={() => onPick(item.action)}>
          {item.label}
        </button>
      ))}
    </div>
  );
}
