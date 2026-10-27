// The list of background jobs of one pane: name, the ports it listens on, how long it has run, a
// short command line and a Stop button that asks once ("Stop this job?"). Shared by the Session
// rail's card and the sheet that lists the jobs of every pane.
import { useEffect, useState } from 'react';
import { openPreview } from '../../../lib/pane-preview';
import { runTime, type PaneJob } from '../../../lib/pane-jobs';
import '../../../styles/cockpit-jobs.css';

/** A clock that moves every `ms` so run times stay fresh between polls. */
function useClock(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return now;
}

const portUrl = (port: number): string => `http://localhost:${port}/`;

function JobRow({ job, now, onStop }: { job: PaneJob; now: number; onStop: (pid: number, startedAtMs: number) => Promise<void> }) {
  const [phase, setPhase] = useState<'idle' | 'asking' | 'stopping'>('idle');
  const [error, setError] = useState<string | null>(null);
  const stop = () => {
    setPhase('stopping');
    setError(null);
    onStop(job.pid, job.startedAtMs)
      .catch((e: unknown) => {
        setError(typeof e === 'string' ? e : e instanceof Error ? e.message : 'Could not stop this job.');
        setPhase('idle');
      });
  };
  return (
    <li className="ck-job" data-testid="background-job">
      <div className="ck-job-top">
        <span className="ck-job-name" title={`Process ${job.pid}`}>
          {job.name}
        </span>
        {job.ports.map((port) => (
          <button key={port} type="button" className="ck-job-port" title={`Open localhost:${port} in your browser`} onClick={() => void openPreview(portUrl(port))}>
            :{port}
          </button>
        ))}
        <span className="ck-job-time" title="How long it has run">
          {runTime(job.startedAtMs, now)}
        </span>
        {phase === 'idle' && (
          <button type="button" className="ck-job-stop" onClick={() => setPhase('asking')} aria-label={`Stop ${job.name}`}>
            Stop
          </button>
        )}
        {phase === 'stopping' && <span className="ck-job-time">Stopping…</span>}
      </div>
      <code className="ck-job-cmd" title={job.command}>
        {job.command}
      </code>
      {phase === 'asking' && (
        <div className="ck-job-ask" role="group" aria-label="Stop this job?">
          <span>Stop this job?</span>
          <button type="button" className="ck-job-stop is-danger" onClick={stop} autoFocus>
            Yes
          </button>
          <button type="button" className="ck-job-stop" onClick={() => setPhase('idle')}>
            No
          </button>
        </div>
      )}
      {error && (
        <p className="ck-job-error" role="alert">
          {error}
        </p>
      )}
    </li>
  );
}

export function BackgroundJobs({ jobs, onStop }: { jobs: readonly PaneJob[]; onStop: (pid: number, startedAtMs: number) => Promise<void> }) {
  const now = useClock(10_000);
  return (
    <ul className="ck-jobs">
      {jobs.map((job) => (
        <JobRow key={job.pid} job={job} now={now} onStop={onStop} />
      ))}
    </ul>
  );
}
