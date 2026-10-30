import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { PaneAction } from '../../lib/pane-actions';
import { IconClose, IconCopy, IconInfo, IconReflow, IconRename, IconStop } from './Icons';

/**
 * The one menu of a pane: opened by a right-click (or the context-menu key, Shift+F10) on its row in the
 * sidebar. Same look as every other menu of the cockpit (`ck-menu`). Groups: view, then the pane's own
 * actions, then Close.
 */
export function PaneContextMenu({
  x,
  y,
  onSide,
  onAction,
  onInterrupt,
  onClose,
}: {
  x: number;
  y: number;
  /** "Open to the side" (split view). Absent: no such item. */
  onSide?: () => void;
  onAction: (action: PaneAction) => void;
  onInterrupt: () => void;
  /** The menu wants to go away (Escape, Tab, a click outside). `refocus` when the keyboard closed it. */
  onClose: (refocus: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });

  // Keep the menu inside the window, then put the keyboard on its first item.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const left = Math.max(4, Math.min(x, window.innerWidth - r.width - 4));
    const top = Math.max(4, Math.min(y, window.innerHeight - r.height - 4));
    setPos({ left, top });
    el.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }, [x, y]);

  useEffect(() => {
    const away = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose(false);
    };
    const blur = () => onClose(false);
    window.addEventListener('mousedown', away);
    window.addEventListener('blur', blur);
    return () => {
      window.removeEventListener('mousedown', away);
      window.removeEventListener('blur', blur);
    };
  }, [onClose]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const items = [...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose(true);
    } else if (e.key === 'Tab') {
      e.preventDefault();
      onClose(true);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      items[(at + 1) % items.length]?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      items[(at - 1 + items.length) % items.length]?.focus();
    } else if (e.key === 'Home') {
      e.preventDefault();
      items[0]?.focus();
    } else if (e.key === 'End') {
      e.preventDefault();
      items[items.length - 1]?.focus();
    }
  };

  const item = (label: string, icon: JSX.Element | null, run: () => void, kbd?: string) => (
    <button
      type="button"
      role="menuitem"
      className="ck-menu-item"
      tabIndex={-1}
      onClick={() => {
        onClose(false);
        run();
      }}
    >
      <span className="ck-menu-ic">{icon}</span>
      <span>{label}</span>
      {kbd && <kbd className="ck-kbd">{kbd}</kbd>}
    </button>
  );

  return (
    <div
      ref={ref}
      className="ck-menu ck-menu--fixed ck-menu--compact"
      role="menu"
      aria-label="Pane actions"
      style={{ left: pos.left, top: pos.top }}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      {onSide && (
        <>
          {item('Open to the side', null, onSide)}
          <div className="ck-menu-sep" role="separator" />
        </>
      )}
      {item('Rename', <IconRename />, () => onAction('rename'))}
      {item('Reflow', <IconReflow />, () => onAction('reflow'))}
      {item('Copy selection', <IconCopy />, () => onAction('copy'))}
      {item('Interrupt', <IconStop />, onInterrupt, '⌃C')}
      {item('Details', <IconInfo />, () => onAction('details'))}
      <div className="ck-menu-sep" role="separator" />
      {item('Close', <IconClose />, () => onAction('close'))}
    </div>
  );
}
