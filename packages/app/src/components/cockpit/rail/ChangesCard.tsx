// Files the agent wrote in this session (not the ones it only read). A click opens the pane's Changes tab when
// it has one; otherwise the list is just a list.
import type { OutputFile } from '../../../lib/sessionChatModel';
import { RailCard } from './RailCard';
import { baseName, fileFolder, filesCount, writtenFiles } from './railFormat';

const SHOWN = 8;

export function ChangesCard({ outputs, cwd, onOpenChanges }: { outputs: readonly OutputFile[]; cwd: string; onOpenChanges?: () => void }) {
  const files = writtenFiles(outputs);
  if (files.length === 0) return null;
  const shown = files.slice(0, SHOWN);
  const rows = shown.map((file) => {
    const folder = fileFolder(file.path, cwd);
    const inner = (
      <>
        <span className="ck-rail-file">{baseName(file.path)}</span>
        {folder && <span className="ck-rail-folder">{folder}</span>}
      </>
    );
    return (
      <li key={file.path}>
        {onOpenChanges ? (
          <button type="button" className="ck-rail-item" title={`${file.path}. Open Changes`} onClick={onOpenChanges}>
            {inner}
          </button>
        ) : (
          <div className="ck-rail-item is-static" title={file.path}>
            {inner}
          </div>
        )}
      </li>
    );
  });
  return (
    <RailCard title="Changes" note={filesCount(files.length)} testId="rail-changes">
      <ul className="ck-rail-list">{rows}</ul>
      {files.length > SHOWN && <p className="ck-rail-more">and {files.length - SHOWN} more</p>}
    </RailCard>
  );
}
