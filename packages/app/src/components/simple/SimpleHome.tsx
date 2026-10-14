// Home in Simple mode: one big button, then her projects. A project is an agent working on a copy;
// the panes behind it are not shown as panes.
import { useEffect, useMemo, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import { cockpitApi, type WorktreeItem } from '../../lib/cockpit-api';
import { DotSpinner } from '../cockpit/DotSpinner';
import { spinnerStatus } from '../cockpit/model';
import { T } from './copy';
import { isOnCopy, projectLabel } from './use-copy';

export interface ProjectRowData {
  paneId: string;
  name: string;
  status: string;
  agent: string;
}

/** Panes on a copy, by pane id. One lookup per pane, again when the panes or their states change. */
export function useCopyMap(c: CockpitApi): Map<string, WorktreeItem> {
  const [map, setMap] = useState<Map<string, WorktreeItem>>(new Map());
  const key = c.registry.paneList.map((p) => `${p.info.id}:${p.status}`).join('|');
  useEffect(() => {
    let cancelled = false;
    Promise.all(
      c.registry.paneList.map((p) =>
        cockpitApi
          .paneWorktree(p.info.id)
          .then((item) => [p.info.id, item] as const)
          .catch(() => [p.info.id, null] as const),
      ),
    ).then((pairs) => {
      if (cancelled) return;
      const next = new Map<string, WorktreeItem>();
      for (const [id, item] of pairs) if (isOnCopy(item)) next.set(id, item as WorktreeItem);
      setMap(next);
    });
    return () => {
      cancelled = true;
    };
    // `key` stands for the list: a new array every render must not start a lookup every time.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return map;
}

export function projectRows(c: CockpitApi, copies: Map<string, WorktreeItem>, agentName: (type: string) => string): ProjectRowData[] {
  return c.registry.paneList
    .filter((p) => copies.has(p.info.id))
    .map((p) => ({
      paneId: p.info.id,
      name: projectLabel(copies.get(p.info.id) ?? null, agentName(p.info.agent_type)),
      status: p.status,
      agent: agentName(p.info.agent_type),
    }));
}

function sentence(status: string): string {
  return status === 'running' ? T.stateWorking : status === 'waiting_input' ? T.stateNeedsYou : status === 'idle' ? T.stateReady : T.stateStopped;
}

export function SimpleHome({ c, onMake, onOpen }: { c: CockpitApi; onMake: () => void; onOpen: (paneId: string) => void }) {
  const listing = useAgentRegistry();
  const copies = useCopyMap(c);
  const rows = useMemo(() => projectRows(c, copies, (type) => agentLabel(listing, type)), [c, copies, listing]);
  const waiting = rows.filter((r) => r.status === 'waiting_input').length;

  return (
    <div className="sx-page sx-home">
      <div className="sx-home-top">
        <h1 className="ck-title">{T.projects}</h1>
        {waiting > 0 && <span className="sx-count" aria-live="polite">{T.needsYouCount} · {waiting}</span>}
      </div>
      <button type="button" className="ck-btn ck-btn--primary sx-make-btn" onClick={onMake}>
        {T.makeSomething}
      </button>
      {rows.length === 0 ? (
        <div className="sx-empty">
          <span aria-hidden="true">
            <DotSpinner status="idle" size={5} />
          </span>
          <p className="sx-lead">{T.noProjects}</p>
        </div>
      ) : (
        <ul className="sx-list" aria-label={T.projects}>
          {rows.map((r) => (
            <li key={r.paneId}>
              <button type="button" className={`sx-row sx-row--button${r.status === 'waiting_input' ? ' is-needs' : ''}`} onClick={() => onOpen(r.paneId)}>
                <span aria-hidden="true">
                  <DotSpinner status={spinnerStatus(r.status)} />
                </span>
                <span className="sx-row-main">
                  <span className="sx-row-title">{r.name}</span>
                  <span className="sx-row-sub">{r.agent}</span>
                </span>
                <span className="sx-row-side">{sentence(r.status)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <p className="sx-foot">
        {T.look} ·{' '}
        <button type="button" className="sx-link" onClick={() => void c.openSettings()}>
          {T.changeLook}
        </button>
      </p>
    </div>
  );
}
