// Split view: two agent chats side by side (like split editors). The left side is the selected pane, the
// right side is the pane opened "to the side". The side with focus is full strength; the other is dimmed.
// The session rail on the right follows the focused side. Each side has its own message box (SessionChatPanel).
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { ScreenQuestionCard } from '../chat/ScreenQuestionCard';
import { SessionChatPanel } from '../chat/SessionChatPanel';
import { composerBlock } from '../../lib/paneOutbox';
import { useAgentRegistry } from '../../lib/agentRegistry';
import { useLoopGuard } from '../../lib/loop-guard';
import { helperOfLabel } from '../../lib/pane-helpers';
import '../../styles/cockpit-split.css';
import { DockedCards } from './DockedCards';
import { DotSpinner } from './DotSpinner';
import { SplitDivider } from './SplitDivider';
import { HelperQuestions, helperQuestionIds } from './HelperQuestions';
import { IconClose, IconRail } from './Icons';
import { dockItems } from './pane-chrome';
import { spinnerStatus, stateText } from './model';
import { SessionRail } from './rail/SessionRail';
import { helperRows } from './rail/helperRows';
import { useRailLayout } from './rail/useRailLayout';
import type { SplitSide } from './main-view';
import { paneHeading } from './titles';

/** Moves the keyboard focus into a side's message box (Ctrl+Alt+Left / Right, see CockpitApp). */
export function focusSideComposer(side: SplitSide) {
  document.querySelector<HTMLTextAreaElement>(`.ck-split-side[data-split-side="${side}"] textarea`)?.focus();
}

/** The window's width, kept current (the split view turns off below a width, see main-view.ts). */
export function useWindowWidth(): number {
  const [width, setWidth] = useState(() => (typeof window === 'undefined' ? 0 : window.innerWidth));
  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  return width;
}

export function SplitView({
  c,
  leftId,
  rightId,
  focus,
  ratio,
  onFocus,
  onRatio,
  onClose,
}: {
  c: CockpitApi;
  leftId: string;
  rightId: string;
  focus: SplitSide;
  /** The left side's share of the room (see main-view.ts). The CSS keeps each side at its minimum width. */
  ratio: number;
  onFocus: (side: SplitSide) => void;
  onRatio: (ratio: number) => void;
  onClose: () => void;
}) {
  const rail = useRailLayout();
  const colsRef = useRef<HTMLDivElement>(null);
  const focusedId = focus === 'right' ? rightId : leftId;
  const fp = c.registry.panes.get(focusedId);
  const agentName = c.names.get(focusedId) ?? fp?.info.agent_type ?? '';
  const listing = useAgentRegistry();
  const agentType = fp?.info.agent_type ?? '';
  const chatAllowed = listing.agents.find((a) => a.id === agentType)?.kind !== 'shell';
  return (
    <div className="ck-split">
      <div ref={colsRef} className="ck-split-cols" style={{ ['--ck-split-ratio' as string]: String(ratio) } as CSSProperties}>
        <SplitSideView c={c} paneId={leftId} side="left" focused={focus === 'left'} onFocus={onFocus} onClose={onClose} />
        <SplitDivider colsRef={colsRef} ratio={ratio} onRatio={onRatio} />
        <SplitSideView
          c={c}
          paneId={rightId}
          side="right"
          focused={focus === 'right'}
          onFocus={onFocus}
          onClose={onClose}
          actions={
            rail.fits ? (
              <button
                type="button"
                className="ck-icon-btn"
                aria-label={rail.open ? 'Hide session details' : 'Show session details'}
                aria-pressed={rail.open}
                title={`${rail.open ? 'Hide' : 'Show'} session details ( ] )`}
                onClick={rail.toggle}
              >
                <IconRail />
              </button>
            ) : null
          }
        />
      </div>
      {rail.visible && fp && (
        <SessionRail
          paneId={focusedId}
          status={fp.status}
          cwd={fp.info.cwd}
          agentName={agentName}
          accountLabel={fp.info.account_label}
          readsChat={chatAllowed}
          helperOf={helperOfLabel(c.registry.paneList, focusedId, (p) => paneHeading(p.info, c.names.get(p.info.id) ?? p.info.agent_type).primary)}
          helpers={helperRows(c.registry.paneList, focusedId, (p) => c.names.get(p.info.id) ?? p.info.agent_type)}
          onOpenPane={(id) => c.select(id)}
        />
      )}
    </div>
  );
}

function SplitSideView({
  c,
  paneId,
  side,
  focused,
  onFocus,
  onClose,
  actions,
}: {
  c: CockpitApi;
  paneId: string;
  side: SplitSide;
  focused: boolean;
  onFocus: (side: SplitSide) => void;
  onClose: () => void;
  /** Extra header buttons (the right side holds the rail toggle and the close button). */
  actions?: ReactNode;
}) {
  const pane = c.registry.panes.get(paneId);
  const listing = useAgentRegistry();
  const { signals } = useLoopGuard();
  const [dockIx, setDockIx] = useState(0);
  if (!pane) return <div className="ck-split-side" />;

  const { info } = pane;
  const name = c.names.get(paneId) ?? info.agent_type;
  const heading = paneHeading(info, name);
  const chatAllowed = listing.agents.find((a) => a.id === info.agent_type)?.kind !== 'shell';
  const prompt = c.prompts[paneId];
  const waiting = pane.status === 'waiting_input';
  // The same cards the Chat tab shows under the conversation, in the same order (see PaneView).
  const asking = waiting && !!prompt && prompt.choices.length > 0;
  const dock = dockItems(asking, !!signals[paneId]);
  const dockNow = dock.length > 0 ? dock[dockIx % dock.length] : null;
  const askScreen = waiting && !asking;
  const helperAsks = helperQuestionIds(c, paneId);
  // "Open Terminal" needs the terminal, which only the single-pane view has: close the split and open this pane.
  const openAlone = () => {
    onClose();
    c.select(paneId);
  };
  const chatDock =
    dockNow || askScreen || helperAsks.length > 0 ? (
      <>
        <HelperQuestions c={c} ids={helperAsks} />
        {dockNow && <DockedCards c={c} paneId={paneId} dock={dock} dockNow={dockNow} onMore={() => setDockIx((i) => i + 1)} />}
        {askScreen && <ScreenQuestionCard paneId={paneId} waiting fallbackText={prompt?.summary || prompt?.risk?.headline} onOpenTerminal={() => openAlone()} />}
      </>
    ) : null;

  return (
    <section
      className={`ck-split-side${side === 'left' ? ' ck-split-side-left' : ''}${focused ? ' is-focused' : ' is-dim'}`}
      data-split-side={side}
      aria-label={`${side === 'left' ? 'Left' : 'Right'} chat: ${heading.primary}`}
      aria-current={focused ? 'true' : undefined}
      onMouseDownCapture={() => onFocus(side)}
      onFocusCapture={() => onFocus(side)}
    >
      <header className="ck-split-head">
        <DotSpinner status={spinnerStatus(pane.status)} />
        <span className="ck-split-title">{heading.primary}</span>
        <span className="ck-split-state">{waiting ? 'Needs you' : stateText(pane.status)}</span>
        {actions}
        {side === 'right' && (
          <button type="button" className="ck-icon-btn" aria-label="Close split view" title="Close split view ( Ctrl+\ )" onClick={onClose}>
            <IconClose />
          </button>
        )}
      </header>
      <div className="ck-scroll ck-fade-in ck-split-body">
        <SessionChatPanel
          paneId={paneId}
          dock={chatDock ?? undefined}
          reply={{ blockedReason: composerBlock(pane, !chatAllowed) }}
          emptyNote={chatAllowed ? undefined : 'A plain shell has no chat. Open it alone to use its terminal.'}
          onOpenPane={(id) => c.select(id)}
        />
      </div>
    </section>
  );
}
