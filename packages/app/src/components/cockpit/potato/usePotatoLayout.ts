// The Potato Bot page's layout as state: list width and strip mode, panel width and open/closed. Read from storage
// once, written back on every change (lib/potato-layout.ts). The page uses the actions; the keys use the toggles.
import { useCallback, useEffect, useState } from 'react';
import {
  clampPanelWidth,
  LIST_DEFAULT,
  LIST_MIN,
  LIST_MINI_WIDTH,
  listFromDrag,
  PANEL_DEFAULT,
  readLayout,
  writeLayout,
  type PotatoLayoutState,
} from '../../../lib/potato-layout';

export interface PotatoLayout extends PotatoLayoutState {
  /** The list's edge was dragged (or keyed) to this width, before limits. Narrow enough: the strip. */
  dragList: (raw: number, via?: 'drag' | 'key') => void;
  resetList: () => void;
  toggleList: () => void;
  /** `room` is the width of the page, so the thread keeps its minimum. */
  dragPanel: (raw: number, room?: number) => void;
  resetPanel: () => void;
  setPanelOpen: (open: boolean) => void;
  togglePanel: () => void;
}

export function usePotatoLayout(initial?: PotatoLayoutState): PotatoLayout {
  const [layout, setLayout] = useState<PotatoLayoutState>(() => initial ?? readLayout());
  useEffect(() => writeLayout(layout), [layout]);

  const dragList = useCallback((raw: number, via: 'drag' | 'key' = 'drag') => {
    setLayout((was) => {
      // From the strip, an arrow key opens the list at its smallest width; a drag follows the pointer.
      const next = was.listMini && via === 'key' && raw > 0 ? { mini: false as const, width: LIST_MIN } : listFromDrag(raw);
      return next.mini ? { ...was, listMini: true } : { ...was, listMini: false, listWidth: next.width };
    });
  }, []);
  const resetList = useCallback(() => setLayout((was) => ({ ...was, listMini: false, listWidth: LIST_DEFAULT })), []);
  const toggleList = useCallback(() => setLayout((was) => ({ ...was, listMini: !was.listMini })), []);
  const dragPanel = useCallback((raw: number, room = 0) => {
    setLayout((was) => ({ ...was, panelWidth: clampPanelWidth(raw, room, was.listMini ? LIST_MINI_WIDTH : was.listWidth) }));
  }, []);
  const resetPanel = useCallback(() => setLayout((was) => ({ ...was, panelWidth: PANEL_DEFAULT })), []);
  const setPanelOpen = useCallback((open: boolean) => setLayout((was) => (was.panelOpen === open ? was : { ...was, panelOpen: open })), []);
  const togglePanel = useCallback(() => setLayout((was) => ({ ...was, panelOpen: !was.panelOpen })), []);

  return { ...layout, dragList, resetList, toggleList, dragPanel, resetPanel, setPanelOpen, togglePanel };
}
