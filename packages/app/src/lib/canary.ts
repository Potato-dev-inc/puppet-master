// Version checks: whether each agent is a version Puppet Master has been tested with, what the
// last zero-token start-up check found, and the screens Puppet Master did not recognise.
// The `canary_*` Tauri commands (src-tauri/src/commands/canary.rs); the status is also pushed as
// the `canary://changed` event.
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type CanaryStanding = 'tested' | 'untested' | 'unpinned' | 'unknown';
export type CanaryOutcome =
  | 'ready'
  | 'asks_you'
  | 'unrecognised'
  | 'never_drew'
  | 'exited'
  | 'start_failed'
  | 'inconclusive';
export type StrictReason = 'untested_version' | 'changed_screens';

export interface CanaryShot {
  label: string;
  screen: string;
}

export interface CanaryResultView {
  outcome: CanaryOutcome;
  passed: boolean;
  /** One plain sentence. */
  summary: string;
  at_ms: number;
  version: string | null;
  screens: CanaryShot[];
}

export interface CanaryAgent {
  agent: string;
  label: string;
  installed: boolean;
  /** `0.162.1`. */
  version: string | null;
  /** The range Puppet Master has tested, `>=0.159, <0.170`. */
  tested: string | null;
  standing: CanaryStanding;
  /** Puppet Master asks before acting for this agent (no start-up notice is closed for you). */
  strict: boolean;
  strict_reason: StrictReason | null;
  /** The chip text, `Untested version`; null when there is nothing to flag. */
  chip: string | null;
  /** Plain sentence for the Settings row. */
  plain: string;
  checking: boolean;
  last: CanaryResultView | null;
  /** Screens saved for this agent that Puppet Master did not recognise. */
  captures: number;
}

export interface CanaryNotice {
  id: string;
  agent: string;
  text: string;
  at_ms: number;
  dismissed: boolean;
}

export interface CanaryStatus {
  agents: CanaryAgent[];
  auto_check: boolean;
  notices: CanaryNotice[];
  captures: number;
}

export const EMPTY_CANARY: CanaryStatus = { agents: [], auto_check: true, notices: [], captures: 0 };

export interface CanaryApi {
  status: () => Promise<CanaryStatus>;
  /** Starts the agent in a scratch folder and quits without sending anything. Takes up to a minute. */
  check: (agent: string) => Promise<CanaryAgent>;
  setAuto: (on: boolean) => Promise<CanaryStatus>;
  dismissNotice: (id: string) => Promise<void>;
  /** Opens the folder of saved screens; resolves with its path. */
  openCaptures: () => Promise<string>;
  onChanged: (cb: (status: CanaryStatus) => void) => Promise<() => void>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const tauriCanaryApi: CanaryApi = {
  status: () => (isTauri() ? invoke<CanaryStatus>('canary_status') : Promise.resolve(EMPTY_CANARY)),
  check: (agent) => invoke<CanaryAgent>('canary_check', { agent }),
  setAuto: (on) => invoke<CanaryStatus>('canary_set_auto', { on }),
  dismissNotice: (id) => invoke('canary_dismiss_notice', { id }),
  openCaptures: () => invoke<string>('canary_open_captures'),
  onChanged: async (cb) => (isTauri() ? listen<CanaryStatus>('canary://changed', (e) => cb(e.payload)) : () => {}),
};

/** The live status: read once, then replaced whenever Rust says it changed. */
export function useCanary(api: CanaryApi = tauriCanaryApi): { status: CanaryStatus; refresh: () => void } {
  const [status, setStatus] = useState<CanaryStatus>(EMPTY_CANARY);
  const refresh = useCallback(() => {
    api.status().then(setStatus).catch(() => {});
  }, [api]);
  useEffect(() => {
    let disposed = false;
    const offs: (() => void)[] = [];
    refresh();
    void api.onChanged(setStatus).then((off) => (disposed ? off() : offs.push(off)));
    return () => {
      disposed = true;
      offs.forEach((off) => off());
    };
  }, [api, refresh]);
  return { status, refresh };
}

/** "3 minutes ago" for the last check line. */
export function checkedAgo(atMs: number, now: number): string {
  const secs = Math.max(0, Math.round((now - atMs) / 1000));
  if (secs < 45) return 'just now';
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins} ${mins === 1 ? 'minute' : 'minutes'} ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours} ${hours === 1 ? 'hour' : 'hours'} ago`;
  const days = Math.round(hours / 24);
  return `${days} ${days === 1 ? 'day' : 'days'} ago`;
}

/** The sentence under an agent in Settings about its last check, or null when it has none. */
export function lastCheckLine(agent: CanaryAgent, now: number): string | null {
  if (!agent.last) return null;
  return `${agent.last.summary} Checked ${checkedAgo(agent.last.at_ms, now)}.`;
}
