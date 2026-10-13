// Bottom sheets of the mobile PWA: "New agent" (opens a pane on the PC) and the "⋯" menu
// (connection, close the current agent, input box settings, classic view link).
import type { ReactNode } from 'react';
import type { AgentType } from '@puppet-master/shared';
import type { LaunchEntry } from '../../lib/agentRegistry';
import type { PublicSettings } from '../../lib/bridge-settings';
import type { BridgeLinkStats } from '../../lib/bridge-sse';

function Sheet({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  return (
    <div className="fixed inset-0 z-50 flex flex-col justify-end bg-black/50" onClick={onClose}>
      <div
        role="dialog"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
        className="max-h-[80dvh] overflow-y-auto rounded-t-xl border-t border-pm-border bg-pm-panel p-4 pb-[calc(1rem+env(safe-area-inset-bottom,0px))] text-xs"
      >
        <div className="mb-3 flex items-center">
          <span className="flex-1 text-sm font-semibold text-zinc-100">{title}</span>
          <button type="button" aria-label="Close" onClick={onClose} className="h-8 w-8 rounded text-lg text-pm-muted active:bg-pm-raised">
            ×
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function NewAgentSheet({
  entries,
  onClose,
  onSpawn,
  spawning,
  error,
}: {
  /** null while the agent list loads. */
  entries: LaunchEntry[] | null;
  onClose: () => void;
  onSpawn: (agentType: AgentType) => void;
  spawning: boolean;
  error: string | null;
}) {
  return (
    <Sheet title="Open an agent on your PC" onClose={onClose}>
      {!entries && <div className="text-pm-muted">Loading agents…</div>}
      <div className="grid grid-cols-2 gap-2">
        {entries?.map((entry) => (
          <button
            key={entry.type}
            type="button"
            disabled={spawning || entry.disabled}
            title={entry.disabledReason}
            onClick={() => onSpawn(entry.type)}
            className="rounded border border-pm-border px-3 py-3 text-left text-zinc-100 active:bg-pm-raised disabled:opacity-40"
          >
            <span className="block truncate">{entry.label}</span>
            {entry.experimental && <span className="block text-[10px] text-pm-warn">experimental</span>}
            {entry.disabledReason && <span className="block text-[10px] text-pm-muted">{entry.disabledReason}</span>}
          </button>
        ))}
      </div>
      {spawning && <div className="mt-3 text-pm-muted">Starting…</div>}
      {error && <div className="mt-3 rounded border border-pm-err/40 bg-pm-err/5 p-2 text-pm-err">{error}</div>}
    </Sheet>
  );
}

/** Keystroke-to-echo delay measured by the phone: the last one and the median of recent ones. */
export interface TypingEchoStats {
  lastMs: number | null;
  medianMs: number | null;
}

const fmtMs = (ms: number | null | undefined): string => (ms === null || ms === undefined ? '—' : `${Math.round(ms)} ms`);

/** One plain-text line describing the link: socket RTT, typing echo, reconnects. */
export function connectionLine(link: BridgeLinkStats | null, echo: TypingEchoStats | null): string {
  const echoPart = `typing echo ${fmtMs(echo?.lastMs)} (median ${fmtMs(echo?.medianMs)})`;
  if (!link) return `Connecting · ${echoPart}`;
  if (link.mode === 'polling') return `Polling (slow) · retrying socket… · ${echoPart}`;
  if (link.mode === 'stream') return `Local stream · ${echoPart}`;
  if (link.mode === 'connecting') return `Connecting · ${echoPart} · reconnects ${link.reconnects}`;
  return `Socket · RTT ${fmtMs(link.rttMs)} (median ${fmtMs(link.rttMedianMs)}) · ${echoPart} · reconnects ${link.reconnects}`;
}

export function MenuSheet({
  onClose,
  bridgeUrl,
  bridgeReady,
  settings,
  onPatchSettings,
  link,
  echo,
  activeLabel,
  onCloseActive,
  onEditConnection,
}: {
  onClose: () => void;
  bridgeUrl: string;
  bridgeReady: boolean;
  settings: PublicSettings;
  onPatchSettings: (patch: Partial<PublicSettings>) => void;
  /** Live link stats from the event stream; null before it has subscribed. */
  link: BridgeLinkStats | null;
  echo: TypingEchoStats | null;
  /** Label of the open agent tab, null on All. */
  activeLabel: string | null;
  onCloseActive: () => void;
  onEditConnection: () => void;
}) {
  const setDelay = (value: number) => {
    const ms = Number.isFinite(value) ? (value <= 0 ? 0 : Math.min(1000, Math.max(50, Math.round(value)))) : 250;
    onPatchSettings({ mobile_input_delay_ms: ms });
  };
  return (
    <Sheet title="Puppet Master" onClose={onClose}>
      <div className="flex items-center gap-2 border-b border-pm-border pb-3">
        <span className={`h-2 w-2 rounded-full ${bridgeReady ? 'bg-pm-ok' : 'bg-pm-warn'}`} />
        <span className="min-w-0 flex-1 truncate text-pm-muted">{bridgeReady ? 'Connected' : 'Connecting'} · {bridgeUrl}</span>
        <button type="button" onClick={onEditConnection} className="rounded border border-pm-border px-2 py-1 text-pm-muted active:bg-pm-raised">
          Change
        </button>
      </div>
      <div className="border-b border-pm-border py-2 font-mono text-[11px] text-pm-muted" aria-label="Connection">
        {connectionLine(link, echo)}
      </div>

      {activeLabel && (
        <div className="border-b border-pm-border py-3">
          <button
            type="button"
            onClick={() => {
              if (window.confirm(`Close ${activeLabel}? This stops the agent on your PC.`)) onCloseActive();
            }}
            className="w-full rounded border border-pm-err/50 px-3 py-2 text-left text-pm-err active:bg-pm-err/10"
          >
            Close {activeLabel}
          </button>
        </div>
      )}

      <div className="space-y-3 py-3">
        <label className="flex items-center justify-between gap-2 text-pm-muted">
          <span>Input box under the terminal</span>
          <input
            type="checkbox"
            checked={settings.mobile_input_visible ?? true}
            disabled={!bridgeReady}
            onChange={(e) => onPatchSettings({ mobile_input_visible: e.target.checked })}
            className="h-4 w-4 accent-pm-accent"
          />
        </label>
        <label className="flex items-start justify-between gap-2 text-pm-muted">
          <span className="flex-1">
            Keyboard resizes the agent
            <span className="block text-[11px] text-pm-muted/80">
              Off: the agent keeps its size; the keyboard covers the top of the screen (swipe to see it). On: the agent redraws above the keyboard (Claude reprints each time).
            </span>
          </span>
          <input
            type="checkbox"
            checked={settings.mobile_keyboard_resizes_agent ?? false}
            disabled={!bridgeReady}
            onChange={(e) => onPatchSettings({ mobile_keyboard_resizes_agent: e.target.checked })}
            className="mt-0.5 h-4 w-4 accent-pm-accent"
          />
        </label>
        <label className="flex items-center gap-2 text-pm-muted">
          <span className="flex-1">Typing buffer</span>
          <input
            type="number"
            min={0}
            max={1000}
            step={10}
            value={settings.mobile_input_delay_ms ?? 60}
            disabled={!bridgeReady}
            onChange={(e) => setDelay(Number(e.target.value))}
            className="w-20 rounded border border-pm-border bg-pm-bg px-2 py-1 font-mono text-pm-text"
          />
          <span>ms</span>
        </label>
      </div>

      <a href="?pwa=legacy" className="block border-t border-pm-border pt-3 text-pm-muted underline">
        Classic mobile view (orchestrator)
      </a>
    </Sheet>
  );
}
