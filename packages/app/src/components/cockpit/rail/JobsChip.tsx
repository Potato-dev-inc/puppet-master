// "3 jobs running" in the sidebar footer: it counts the background jobs of every pane and opens a
// sheet that lists them grouped by pane. Hidden when nothing runs in the background.
import { useEffect, useState } from 'react';
import { jobCount, jobsRunning, tauriJobsApi, usePaneJobs, type JobsApi, type JobsMap } from '../../../lib/pane-jobs';
import { IconTerminal } from '../Icons';
import { Sheet } from '../Sheet';
import { stopJob } from './BackgroundCard';
import { BackgroundJobs } from './BackgroundJobs';

export interface JobsChipProps {
  /** Pane id to the name shown for it. */
  names: ReadonlyMap<string, string>;
  /** Open a pane (the sheet closes first). */
  onOpenPane: (id: string) => void;
  api?: JobsApi;
}

export function JobsSheet({ jobs, names, onOpenPane, onClose, api = tauriJobsApi }: { jobs: JobsMap; onClose: () => void } & JobsChipProps) {
  const groups = Object.entries(jobs).filter(([, list]) => list.length > 0);
  // Nothing left to show (the last job stopped): the sheet goes away on its own.
  useEffect(() => {
    if (groups.length === 0) onClose();
  }, [groups.length, onClose]);
  return (
    <Sheet labelledBy="ck-jobs-t" role="dialog" onClose={onClose} className="ck-sheet--jobs">
      <h2 className="ck-section" id="ck-jobs-t">
        Background
      </h2>
      <p className="ck-sub">Things the agents started and left running.</p>
      <div className="ck-jobs-groups">
        {groups.map(([paneId, list]) => (
          <section key={paneId} className="ck-jobs-group" aria-label={names.get(paneId) ?? 'Agent'}>
            <div className="ck-jobs-group-h">
              <span>{names.get(paneId) ?? 'Agent'}</span>
              <button
                type="button"
                className="ck-job-stop"
                onClick={() => {
                  onClose();
                  onOpenPane(paneId);
                }}
              >
                Open
              </button>
            </div>
            <BackgroundJobs jobs={list} onStop={(pid, startedAtMs) => stopJob(api, paneId, pid, startedAtMs)} />
          </section>
        ))}
      </div>
      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--secondary" onClick={onClose}>
          Close
        </button>
      </div>
    </Sheet>
  );
}

export function JobsChip({ names, onOpenPane, api = tauriJobsApi }: JobsChipProps) {
  const jobs = usePaneJobs(api);
  const [open, setOpen] = useState(false);
  const count = jobCount(jobs);
  if (count === 0) return null;
  return (
    <>
      <button type="button" className="ck-btn ck-btn--plain ck-jobs-chip" onClick={() => setOpen(true)} data-testid="jobs-chip" title="Things agents left running">
        <IconTerminal />
        {jobsRunning(count)}
      </button>
      {open && <JobsSheet jobs={jobs} names={names} onOpenPane={onOpenPane} onClose={() => setOpen(false)} api={api} />}
    </>
  );
}
