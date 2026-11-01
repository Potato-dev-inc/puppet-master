// The saved versions of a bot's instructions, newest first. The top one is the text in use; the others have Restore
// (which saves the old text again as the newest version). A row expands to show the whole text.
import { useState } from 'react';
import { ADVANCED_COPY, versionPreview, versionWhen, type InstructionVersion } from '../../../lib/potato-botsettings';

export interface InstructionHistoryProps {
  versions: InstructionVersion[];
  onRestore: (at: number) => void;
}

export function InstructionHistory({ versions, onRestore }: InstructionHistoryProps) {
  const [open, setOpen] = useState<number | null>(null);
  if (versions.length === 0) return <p className="ck-potato-adv-line">{ADVANCED_COPY.noHistory}</p>;
  return (
    <ol className="ck-potato-adv-list ck-potato-adv-history" aria-label={ADVANCED_COPY.history}>
      {versions.map((version, index) => {
        const current = index === 0;
        const expanded = open === version.at;
        return (
          <li key={version.at} className="ck-potato-adv-version">
            <div className="ck-potato-adv-row">
              <span className="ck-potato-adv-version-when">{versionWhen(version.at)}</span>
              {current ? (
                <span className="ck-potato-adv-pill">{ADVANCED_COPY.inUse}</span>
              ) : (
                <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={() => onRestore(version.at)}>
                  {ADVANCED_COPY.restore}
                </button>
              )}
            </div>
            <div className="ck-potato-adv-row">
              <span className="ck-potato-adv-version-text">{versionPreview(version.text) || '(empty)'}</span>
              <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" aria-expanded={expanded} onClick={() => setOpen(expanded ? null : version.at)}>
                {expanded ? ADVANCED_COPY.hide : ADVANCED_COPY.show}
              </button>
            </div>
            {expanded && <pre className="ck-potato-adv-full">{version.text || '(empty)'}</pre>}
          </li>
        );
      })}
    </ol>
  );
}
