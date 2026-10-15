// A mission's state glyph: a mission is not a pane, so it picks one of the five patterns by meaning
// (UX-PHASE1 0.1): (~) happening, (+) needs a person, (:) waiting, (x) failed, (v) finished.
import type { Glyph } from '../../../lib/missions';
import { DotSpinner } from '../DotSpinner';

const STATUS: Record<Glyph, 'running' | 'waiting_input' | 'idle' | 'error' | 'exited'> = {
  '~': 'running',
  '+': 'waiting_input',
  ':': 'idle',
  x: 'error',
  v: 'exited',
};

export function MissionGlyph({ glyph, size }: { glyph: Glyph; size?: 3 | 5 }) {
  return (
    <span className="ck-mission-glyph" data-glyph={glyph}>
      <DotSpinner status={STATUS[glyph] ?? 'idle'} size={size} />
    </span>
  );
}

/** The plain-text "Preview" tag every mission surface carries (STYLE: an 11px label-2 word, no fill). */
export function PreviewTag() {
  return <span className="ck-preview-tag">Preview</span>;
}
