// The agent picker: choose another agent for a companion, optionally say when it goes back, and press Move. Used by the
// Setup card's Agent row and by the "Wake {name} on…" sheet on a usage-limit card. It only collects the choice; the
// caller sends it (`companion_update` with a new `agentType`, which waits while the companion is busy).
import { useMemo, useState } from 'react';
import type { CompanionAgentOption } from '../../../lib/companion';
import { clockValue, nextClockMs, parseClock } from './model';

export interface MovePickerProps {
  /** The agent it is on now (a move to it is no move). */
  currentAgent: string;
  agents: CompanionAgentOption[];
  /** Preselect this agent (the sheet picks another one than the current). */
  initialAgent?: string;
  /** Pre-fill "until" (epoch ms), e.g. the moment the usage limit ends. */
  defaultUntilMs?: number | null;
  submitLabel: string;
  busy?: boolean;
  /** Reads the clock (the tests pin it). */
  now?: () => number;
  onMove: (agentType: string, untilMs: number | null) => void;
}

export function MovePicker({ currentAgent, agents, initialAgent, defaultUntilMs, submitLabel, busy, now = Date.now, onMove }: MovePickerProps) {
  const [agent, setAgent] = useState(initialAgent ?? currentAgent);
  const [until, setUntil] = useState(defaultUntilMs ? clockValue(defaultUntilMs) : '');
  const picked = agents.find((a) => a.id === agent);
  const differs = agent !== currentAgent;
  const at = until === '' ? null : parseClock(until);
  const timeOk = until === '' || at !== null;
  const canMove = differs && !busy && timeOk && (picked?.ok ?? false);
  const options = useMemo(() => (agents.some((a) => a.id === currentAgent) ? agents : [{ id: currentAgent, label: currentAgent, ok: true, reason: null }, ...agents]), [agents, currentAgent]);

  return (
    <div className="ck-companion-move">
      <select className="ck-companion-input" aria-label="Agent" value={agent} disabled={busy} onChange={(e) => setAgent(e.target.value)}>
        {options.map((a) => (
          <option key={a.id} value={a.id} disabled={!a.ok && a.id !== currentAgent}>
            {a.label}
          </option>
        ))}
      </select>
      {differs && picked && !picked.ok && <p className="ck-sub ck-companion-move-reason">{picked.reason ?? "Chat can't send messages to this agent yet."}</p>}
      {differs && (
        <div className="ck-companion-move-row">
          <label className="ck-companion-move-until">
            <span className="ck-caption">Until</span>
            <input className="ck-companion-input" type="time" aria-label="Until (optional)" value={until} onChange={(e) => setUntil(e.target.value)} />
          </label>
          <button type="button" className="ck-btn ck-btn--sm ck-btn--primary" disabled={!canMove} onClick={() => onMove(agent, at ? nextClockMs(at, now()) : null)}>
            {submitLabel}
          </button>
        </div>
      )}
      {differs && <p className="ck-sub ck-companion-move-hint">{until === '' ? 'It stays on the new agent until you move it back.' : 'Then it goes back to the agent it is on now.'}</p>}
    </div>
  );
}
