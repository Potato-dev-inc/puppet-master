// The files chosen for the next message, as small chips above the box: a little picture or a file glyph, the name, the
// size, and a button to take it off again.
import { sizeWord, type Attachment } from '../../../lib/potato-composer';

function FileGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 1.8h5l3 3V14a.2.2 0 0 1-.2.2H4a.2.2 0 0 1-.2-.2V2a.2.2 0 0 1 .2-.2z" />
      <path d="M9 1.8V5h3" />
    </svg>
  );
}

export interface AttachChipsProps {
  files: readonly Attachment[];
  onRemove: (id: string) => void;
}

export function AttachChips({ files, onRemove }: AttachChipsProps) {
  if (files.length === 0) return null;
  return (
    <ul className="ck-potato-chips" aria-label="Attached files">
      {files.map((f) => (
        <li key={f.id} className="ck-potato-chip">
          {f.kind === 'image' && f.preview ? <img className="ck-potato-chip-thumb" src={f.preview} alt="" /> : <span className="ck-potato-chip-glyph"><FileGlyph /></span>}
          <span className="ck-potato-chip-name" title={f.name}>
            {f.name}
          </span>
          <span className="ck-potato-chip-size">{sizeWord(f.size)}</span>
          <button type="button" className="ck-potato-chip-x" aria-label={`Remove ${f.name}`} onClick={() => onRemove(f.id)}>
            <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6" />
            </svg>
          </button>
        </li>
      ))}
    </ul>
  );
}
