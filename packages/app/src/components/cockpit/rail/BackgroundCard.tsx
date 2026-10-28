// The "Background" card of the Session rail: what the agent left running in this pane. Hidden when
// nothing is.
import { refreshJobs, tauriJobsApi, type JobsApi, type PaneJob } from '../../../lib/pane-jobs';
import { openPreview } from '../../../lib/pane-preview';
import { pointApi } from '../../../lib/point-api';
import { BackgroundJobs } from './BackgroundJobs';
import { RailCard } from './RailCard';

/** Stop `pid` of `paneId` (the process that started at `startedAtMs`), then look again at once so the row goes away without waiting. */
export const stopJob = (api: JobsApi, paneId: string, pid: number, startedAtMs: number): Promise<void> =>
  api.stop(paneId, pid, startedAtMs).then(() => refreshJobs());

/** A port of one of `paneId`'s jobs: the in-app preview (`armed` starts the picker), or your browser when `external`. */
export const previewJobPort = (paneId: string, port: number, external: boolean, armed = false): Promise<void> =>
  external ? openPreview(`http://localhost:${port}/`) : pointApi.openPreview(paneId, port, armed).then(() => undefined);

export function BackgroundCard({ paneId, jobs, api = tauriJobsApi }: { paneId: string; jobs: readonly PaneJob[]; api?: JobsApi }) {
  if (jobs.length === 0) return null;
  return (
    <RailCard title="Background" note={String(jobs.length)} testId="rail-background">
      <BackgroundJobs
        jobs={jobs}
        onStop={(pid, startedAtMs) => stopJob(api, paneId, pid, startedAtMs)}
        onPreview={(port, external) => previewJobPort(paneId, port, external)}
        onPointFix={(port) => previewJobPort(paneId, port, false, true)}
      />
    </RailCard>
  );
}
