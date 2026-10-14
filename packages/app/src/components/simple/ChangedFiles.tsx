// "What changed" in Simple mode: the files in her copy, in plain words. The data comes from the
// Changes component (the same worktree diff); this only draws it. No branch, no diff, no counts of lines.
import type { WorktreeFile } from '../../lib/cockpit-api';
import { T } from './copy';

export function changeWord(status: WorktreeFile['status']): string {
  return status === 'added' ? T.changedNew : status === 'deleted' ? T.changedRemoved : T.changedEdited;
}

export function ChangedFiles({ files, working, loaded }: { files: WorktreeFile[]; working: boolean; loaded: boolean }) {
  return (
    <section className="sx-changed" aria-label={T.tabChanged}>
      <p className="sx-lead">{T.changedIntro}</p>
      {loaded && files.length === 0 ? (
        <p className="sx-quiet">{working ? T.thinking : T.changedEmpty}</p>
      ) : (
        <ul className="sx-list">
          {files.map((f) => (
            <li key={f.path} className="sx-row">
              <span className="sx-row-main">{f.path}</span>
              <span className="sx-row-side">{changeWord(f.status)}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
