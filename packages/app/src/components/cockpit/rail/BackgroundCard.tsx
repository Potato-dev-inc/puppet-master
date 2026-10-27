// The "Background" card of the Session rail: what the agent left running in this pane. Hidden when
// nothing is.
import { refreshJobs, tauriJobsApi, type JobsApi, type PaneJob } from '../../../lib/pane-jobs';
import { BackgroundJobs } from './BackgroundJobs';
import { RailCard } from './RailCard';

/** Stop `pid` of `paneId`, then look again at once so the row goes away without waiting. */
export const stopJob = (api: JobsApi, paneId: string, pid: number): Promise<void> => api.stop(paneId, pid).then(() => refreshJobs());

export function BackgroundCard({ paneId, jobs, api = tauriJobsApi }: { paneId: string; jobs: readonly PaneJob[]; api?: JobsApi }) {
  if (jobs.length === 0) return null;
  return (
    <RailCard title="Background" note={String(jobs.length)} testId="rail-background">
      <BackgroundJobs jobs={jobs} onStop={(pid) => stopJob(api, paneId, pid)} />
    </RailCard>
  );
}
