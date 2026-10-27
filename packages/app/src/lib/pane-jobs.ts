// Background jobs: the long-lived processes an agent left running inside a pane (a dev server, a
// watcher, a background shell). Backend: `pane_jobs_all` / `pane_job_stop` (src-tauri pane_jobs/).
// One poll serves every card and chip on screen: it runs every 3 s while something listens and the
// window is visible, and stops when nothing does.
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { formatElapsed } from '../components/cockpit/model';

export interface PaneJob {
  /** The process to stop; its whole tree goes with it. */
  pid: number;
  parentPid: number;
  name: string;
  /** Short command line, already redacted. */
  command: string;
  startedAtMs: number;
  ports: number[];
}

/** Jobs per pane id; panes with none are left out. */
export type JobsMap = Record<string, PaneJob[]>;

export const POLL_MS = 3000;

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export interface JobsApi {
  all: () => Promise<JobsMap>;
  /** `startedAtMs` is the job's own start time: the backend stops the pid only while it is still that process. */
  stop: (paneId: string, pid: number, startedAtMs: number) => Promise<void>;
}

export const tauriJobsApi: JobsApi = {
  all: async () => (inTauri() ? invoke<JobsMap>('pane_jobs_all') : {}),
  stop: (paneId, pid, startedAtMs) => invoke<void>('pane_job_stop', { paneId, pid, startedAtMs }),
};

/** "12m", "3h 5m": how long a job has run. */
export const runTime = (startedAtMs: number, now: number): string => formatElapsed(now - startedAtMs);

export const jobCount = (map: JobsMap): number => Object.values(map).reduce((sum, list) => sum + list.length, 0);

/** "1 job running", "3 jobs running". */
export const jobsRunning = (n: number): string => `${n} ${n === 1 ? 'job' : 'jobs'} running`;

const same = (a: JobsMap, b: JobsMap): boolean => JSON.stringify(a) === JSON.stringify(b);

// --- the shared poll ---------------------------------------------------------------------------
let current: JobsMap = {};
const listeners = new Set<(map: JobsMap) => void>();
let timer: ReturnType<typeof setInterval> | undefined;
let busy = false;
let api: JobsApi = tauriJobsApi;

async function refresh(): Promise<void> {
  if (busy || (typeof document !== 'undefined' && document.visibilityState === 'hidden')) return;
  busy = true;
  try {
    const next = (await api.all()) ?? {};
    if (!same(current, next)) {
      current = next;
      listeners.forEach((fn) => fn(next));
    }
  } catch {
    // The next tick asks again.
  } finally {
    busy = false;
  }
}

/** Ask now (after a stop), without waiting for the next tick. */
export const refreshJobs = (): Promise<void> => refresh();

/** All jobs per pane. `source` is for tests. */
export function usePaneJobs(source: JobsApi = tauriJobsApi): JobsMap {
  const [map, setMap] = useState<JobsMap>(current);
  useEffect(() => {
    api = source;
    listeners.add(setMap);
    setMap(current);
    if (listeners.size === 1) {
      void refresh();
      timer = setInterval(() => void refresh(), POLL_MS);
    } else void refresh();
    return () => {
      listeners.delete(setMap);
      if (listeners.size === 0) {
        if (timer) clearInterval(timer);
        timer = undefined;
        current = {};
      }
    };
  }, [source]);
  return map;
}
