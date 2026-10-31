// A helper request in the bot's thread (spec section 2.2): "Fish wants a helper: …", the folder, the agent, and three
// buttons. Nothing starts without Start. Change folder lists the recent projects by name; the chosen name goes with
// Start, and the app matches it against the recent list again (a path is never taken from here or from the bot).
import { useState } from 'react';
import { plainError } from '../../../lib/potato';

export interface HelperCardProps {
  name: string;
  task: string;
  /** The folder name the bot asked for, if it said one. */
  project?: string;
  /** The label of the agent that would do the work. */
  agentLabel: string;
  /** False once somebody answered: the buttons are gone. */
  open: boolean;
  /** Start, with the folder the person picked here (a name), if any. */
  onStart: (folder?: string) => Promise<void>;
  onNotNow: () => Promise<void>;
  /** The recent project names for Change folder. */
  loadFolders: () => Promise<string[]>;
}

export function HelperCard({ name, task, project, agentLabel, open, onStart, onNotNow, loadFolders }: HelperCardProps) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [choosing, setChoosing] = useState(false);
  const [folders, setFolders] = useState<string[] | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const shown = picked ?? project;

  const run = async (call: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setProblem(null);
    try {
      await call();
    } catch (error) {
      setProblem(plainError(error, 'That did not work. Try again.'));
    } finally {
      setBusy(false);
    }
  };

  const changeFolder = () =>
    run(async () => {
      if (choosing) {
        setChoosing(false);
        return;
      }
      setFolders(await loadFolders());
      setChoosing(true);
    });

  return (
    <section className={`ck-potato-card${open ? '' : ' is-closed'}`} aria-label={`${name} wants a helper`}>
      <p className="ck-potato-card-title">
        {name} wants a helper: <span className="ck-potato-card-task">{task}</span>
      </p>
      <p className="ck-sub ck-potato-card-fact">
        Folder: <span className="ck-mono">{shown || 'not chosen yet'}</span>
      </p>
      <p className="ck-sub ck-potato-card-fact">Agent: {agentLabel}</p>
      {open && choosing && folders && (
        <div className="ck-potato-card-folders" role="group" aria-label="Recent folders">
          {folders.length === 0 ? (
            <p className="ck-sub">No recent folders yet. Open a project first, then try again.</p>
          ) : (
            folders.map((folder) => (
              <button
                key={folder}
                type="button"
                className={`ck-btn ck-btn--plain ck-btn--sm${folder === shown ? ' is-picked' : ''}`}
                aria-pressed={folder === shown}
                onClick={() => {
                  setPicked(folder);
                  setChoosing(false);
                }}
              >
                {folder}
              </button>
            ))
          )}
        </div>
      )}
      {open && (
        <div className="ck-potato-card-actions">
          <button type="button" className="ck-btn ck-btn--primary" disabled={busy} onClick={() => void run(() => onStart(picked ?? undefined))}>
            Start
          </button>
          <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={() => void changeFolder()}>
            Change folder
          </button>
          <button type="button" className="ck-btn ck-btn--plain" disabled={busy} onClick={() => void run(onNotNow)}>
            Not now
          </button>
        </div>
      )}
      {problem && (
        <p className="ck-sub ck-potato-card-problem" role="status">
          {problem}
        </p>
      )}
    </section>
  );
}
