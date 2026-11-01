// The three regions of the Potato Bot page: the bot list on the left, the thread in the middle and, when it is open,
// the bot panel on the right. The list and the panel are resizable (ResizeHandle); their sizes live in `layout`.
// This component only places things: what goes in each slot is up to the page.
import { useRef, type CSSProperties, type ReactNode } from 'react';
import { LIST_MAX, LIST_MIN, listShownWidth, PANEL_MAX, PANEL_MIN } from '../../../lib/potato-layout';
import { LIST_COPY } from '../../../lib/potato-list';
import { ResizeHandle } from './ResizeHandle';
import type { PotatoLayout } from './usePotatoLayout';
import '../../../styles/cockpit-potato.css';

export interface PotatoLayoutProps {
  layout: PotatoLayout;
  list: ReactNode;
  thread: ReactNode;
  /** The panel's contents. Null: no panel slot at all (the panel is closed). */
  panel: ReactNode | null;
}

export function PotatoLayout({ layout, list, thread, panel }: PotatoLayoutProps) {
  const page = useRef<HTMLDivElement>(null);
  const shown = listShownWidth(layout);
  const style = {
    '--potato-list-w': `${layout.listWidth}px`,
    '--potato-list-shown': `${shown}px`,
    '--potato-panel-w': `${layout.panelWidth}px`,
  } as CSSProperties;

  return (
    <div className={`ck-potato-page${layout.listMini ? ' is-mini' : ''}`} ref={page} style={style}>
      {list}
      <ResizeHandle
        label={LIST_COPY.resize}
        grows="right"
        value={shown}
        min={LIST_MIN}
        max={LIST_MAX}
        onChange={layout.dragList}
        onReset={layout.resetList}
      />
      <div className="ck-potato-main">{thread}</div>
      {panel !== null && (
        <>
          <ResizeHandle
            label="Resize the bot panel"
            grows="left"
            value={layout.panelWidth}
            min={PANEL_MIN}
            max={PANEL_MAX}
            onChange={(raw) => layout.dragPanel(raw, page.current?.clientWidth ?? 0)}
            onReset={layout.resetPanel}
          />
          <aside className="ck-potato-panel" aria-label="Bot panel">
            {panel}
          </aside>
        </>
      )}
    </div>
  );
}
