// The pane title in the cockpit header: double-click (or ⋯ > Rename) to edit in place.
// Enter saves, Escape cancels, a blank name clears it so automatic titling resumes.
import { useEffect, useRef, useState } from 'react';
import { cleanTitleInput, MAX_TITLE_CHARS } from './titles';

export interface RenameTitleProps {
  text: string;
  /** The user's own title (or the derived one) when there is one; the editor starts from it. */
  current: string;
  editing: boolean;
  onEditingChange: (editing: boolean) => void;
  onSave: (title: string) => void;
}

export function RenameTitle({ text, current, editing, onEditingChange, onSave }: RenameTitleProps) {
  const [draft, setDraft] = useState(current);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) {
      setDraft(current);
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editing, current]);

  if (!editing) {
    return (
      <h1
        className="ck-title ck-title--sm ck-title--editable"
        title="Double-click to rename"
        onDoubleClick={() => onEditingChange(true)}
      >
        {text}
      </h1>
    );
  }

  const commit = () => {
    onEditingChange(false);
    const next = cleanTitleInput(draft);
    if (next !== current) onSave(next);
  };

  return (
    <input
      ref={inputRef}
      className="ck-title-input"
      aria-label="Pane name"
      value={draft}
      maxLength={MAX_TITLE_CHARS}
      placeholder="Name this pane"
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          commit();
        } else if (e.key === 'Escape') {
          e.preventDefault();
          onEditingChange(false);
        }
      }}
    />
  );
}
