// One proposed skill in "{name} wants to remember" (SPEC-1028 section 6, skills): the title, the steps, the same safety
// tags as a memory note, and Approve / Edit / Reject. Nothing becomes a skill until the person approves it here.
import { useState } from 'react';
import { SKILL_CAP, type PendingItem } from '../../../lib/companion';
import { SKILL_TITLE_MAX, formatCount, skillFileChars } from './model';

export function SkillProposalRow({
  item,
  disabled,
  onApprove,
  onReject,
}: {
  item: PendingItem;
  disabled: boolean;
  /** `edit` is set when the person changed the title or the steps first. */
  onApprove: (item: PendingItem, edit?: { title: string; body: string }) => void;
  onReject: (item: PendingItem) => void;
}) {
  const body = item.body ?? '';
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(item.text);
  const [steps, setSteps] = useState(body);

  const chars = skillFileChars(title, steps);
  const over = chars > SKILL_CAP;
  const empty = title.trim() === '' || steps.trim() === '';
  const tooLong = skillFileChars(item.text, body) > SKILL_CAP;

  return (
    <li className="ck-companion-pending ck-skill-proposal" data-kind="skill">
      <span className="ck-skill-kind">New skill</span>
      {editing ? (
        <>
          <input
            className="ck-companion-input ck-skill-title-input"
            aria-label="Skill title"
            value={title}
            maxLength={SKILL_TITLE_MAX}
            onChange={(e) => setTitle(e.target.value)}
          />
          <textarea
            className="ck-memory-editor ck-companion-editor ck-skill-editor"
            aria-label="Skill steps"
            value={steps}
            onChange={(e) => setSteps(e.target.value)}
            spellCheck={false}
          />
          <span className={`ck-sub${over ? ' ck-memory-err' : ''}`} aria-live="polite">
            {formatCount(chars)} / {formatCount(SKILL_CAP)}
          </span>
        </>
      ) : (
        <>
          <p className="ck-companion-pending-text ck-skill-title">{item.text}</p>
          <pre className="ck-skill-body">{body}</pre>
          {tooLong && <p className="ck-sub ck-memory-err">Too long for a skill. Edit it to make it shorter.</p>}
        </>
      )}
      {item.flagged && <span className="ck-memory-tag">{item.flagged}</span>}
      <span className="ck-companion-pending-actions">
        {editing ? (
          <>
            <button
              type="button"
              className="ck-btn ck-btn--sm ck-btn--secondary"
              disabled={disabled || over || empty}
              onClick={() => onApprove(item, { title: title.trim(), body: steps.trim() })}
            >
              Approve
            </button>
            <button
              type="button"
              className="ck-btn ck-btn--sm ck-btn--plain"
              disabled={disabled}
              onClick={() => {
                setEditing(false);
                setTitle(item.text);
                setSteps(body);
              }}
            >
              Cancel
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="ck-btn ck-btn--sm ck-btn--secondary"
              disabled={disabled || tooLong}
              onClick={() => onApprove(item)}
            >
              Approve
            </button>
            <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" disabled={disabled} onClick={() => setEditing(true)}>
              Edit
            </button>
            <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" disabled={disabled} onClick={() => onReject(item)}>
              Reject
            </button>
          </>
        )}
      </span>
    </li>
  );
}
