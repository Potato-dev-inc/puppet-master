import { useEffect, useRef } from 'react';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import type { SpawnState } from '../../hooks/useCockpit';
import { DotSpinner } from './DotSpinner';
import { spawnLogLine } from './model';

export function SpawnView({ spawn, onDismiss }: { spawn: SpawnState; onDismiss: () => void }) {
  const listing = useAgentRegistry();
  const logRef = useRef<HTMLOListElement>(null);
  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [spawn.log.length]);
  const label = agentLabel(listing, spawn.agentId);

  return (
    <div className="ck-spawn">
      <div className="ck-spawn-glyph">
        <DotSpinner status={spawn.failed ? 'error' : 'running'} size={5} />
      </div>
      <h1 className="ck-title">
        {spawn.failed ? 'Could not start' : 'Starting'} {label}
      </h1>
      <ol className="ck-spawn-log" ref={logRef} aria-live="polite">
        {spawn.log.map((l, i) => (
          <li key={i} className={i === spawn.log.length - 1 ? 'is-last' : undefined}>
            {spawnLogLine(
              {
                agent_id: spawn.agentId,
                pane_id: spawn.paneId,
                stage: l.stage,
                warm: l.warm,
                message: l.message,
              },
              label,
            )}
          </li>
        ))}
        {spawn.log.length === 0 && !spawn.failed && <li>Getting started</li>}
      </ol>
      {spawn.failed && (
        <>
          <p className="ck-sub">{spawn.failed}</p>
          <button type="button" className="ck-btn ck-btn--secondary" onClick={onDismiss}>
            Dismiss
          </button>
        </>
      )}
    </div>
  );
}
