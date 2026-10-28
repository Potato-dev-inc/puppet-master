// The "Skills" tab of "What {name} knows": the skills it has learned, one tap to read, edit or remove. A skill is a
// plain Markdown file in the home folder (skills/<name>.md). Remove moves the file to skills/.removed/, it is not deleted.
import { useState } from 'react';
import { MAX_SKILLS, SKILL_CAP, type CompanionTransport, type MemoryView, type SkillFull, type SkillRef } from '../../../lib/companion';
import { SKILL_TITLE_MAX, formatCount, skillFileChars } from './model';

export function SkillsPanel({
  companionId,
  name,
  skills,
  transport,
  onChanged,
}: {
  companionId: string;
  name: string;
  skills: SkillRef[];
  transport: CompanionTransport;
  onChanged: (next: MemoryView) => void;
}) {
  const [open, setOpen] = useState<SkillFull | null>(null);
  const [draft, setDraft] = useState<{ title: string; body: string } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Something went wrong.');
    } finally {
      setBusy(false);
    }
  };

  const close = () => {
    setOpen(null);
    setDraft(null);
    setConfirmRemove(false);
    setError(null);
  };

  const openSkill = (slug: string) => run(async () => setOpen(await transport.getSkill(companionId, slug)));

  const save = (skill: SkillFull, title: string, body: string) =>
    run(async () => {
      onChanged(await transport.saveSkill(companionId, skill.slug, title, body));
      close();
    });

  const remove = (skill: SkillFull) =>
    run(async () => {
      onChanged(await transport.removeSkill(companionId, skill.slug));
      close();
    });

  if (open) {
    const chars = draft ? skillFileChars(draft.title, draft.body) : open.chars;
    const over = chars > SKILL_CAP;
    return (
      <div className="ck-skills" data-testid="skill-open">
        {open.unreviewed && (
          <div className="ck-companion-notice" role="status">
            <span>This skill changed outside Puppet Master. {name} will not list it until you keep it.</span>
            <button
              type="button"
              className="ck-btn ck-btn--sm ck-btn--secondary"
              disabled={busy}
              onClick={() => void save(open, open.title, open.body)}
            >
              Keep it
            </button>
          </div>
        )}
        {draft ? (
          <>
            <input
              className="ck-companion-input ck-skill-title-input"
              aria-label="Skill title"
              value={draft.title}
              maxLength={SKILL_TITLE_MAX}
              onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            />
            <textarea
              className="ck-memory-editor ck-companion-editor"
              aria-label="Skill steps"
              value={draft.body}
              onChange={(e) => setDraft({ ...draft, body: e.target.value })}
              spellCheck={false}
            />
          </>
        ) : (
          <>
            <p className="ck-companion-pending-text ck-skill-title">{open.title}</p>
            <pre className="ck-skill-body">{open.body}</pre>
          </>
        )}
        {open.flagged && <span className="ck-memory-tag">{open.flagged}</span>}
        <div className="ck-companion-meta">
          <span className={over ? 'ck-memory-err' : undefined} aria-live="polite">
            {formatCount(chars)} / {formatCount(SKILL_CAP)}
          </span>
          <span className="ck-companion-meta-actions">
            {draft ? (
              <>
                <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" disabled={busy} onClick={() => setDraft(null)}>
                  Cancel
                </button>
                <button
                  type="button"
                  className="ck-btn ck-btn--sm ck-btn--primary"
                  disabled={busy || over || draft.title.trim() === '' || draft.body.trim() === ''}
                  onClick={() => void save(open, draft.title.trim(), draft.body.trim())}
                >
                  Save
                </button>
              </>
            ) : confirmRemove ? (
              <>
                <span>Remove it?</span>
                <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" disabled={busy} onClick={() => setConfirmRemove(false)}>
                  No
                </button>
                <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" disabled={busy} onClick={() => void remove(open)}>
                  Yes, remove
                </button>
              </>
            ) : (
              <>
                <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" onClick={close}>
                  Back
                </button>
                <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" onClick={() => setConfirmRemove(true)}>
                  Remove
                </button>
                <button
                  type="button"
                  className="ck-btn ck-btn--sm ck-btn--secondary"
                  onClick={() => setDraft({ title: open.title, body: open.body })}
                >
                  Edit
                </button>
              </>
            )}
          </span>
        </div>
        {error && (
          <p className="ck-memory-msg ck-memory-err" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="ck-skills">
      {skills.length === 0 ? (
        <p className="ck-sub ck-skills-empty">No skills yet. When {name} learns a way you like things done, it will ask you here.</p>
      ) : (
        <ul className="ck-companion-list ck-skills-list">
          {skills.map((skill) => (
            <li key={skill.slug} className="ck-skills-row">
              <button type="button" className="ck-skills-open" disabled={busy} onClick={() => void openSkill(skill.slug)}>
                <span className="ck-skills-name">{skill.title}</span>
                <span className="ck-sub">{formatCount(skill.chars)} characters</span>
              </button>
              {skill.unreviewed && <span className="ck-memory-tag">Changed outside Puppet Master</span>}
              {skill.flagged && <span className="ck-memory-tag">{skill.flagged}</span>}
            </li>
          ))}
        </ul>
      )}
      <div className="ck-companion-meta">
        <span aria-live="polite">
          {skills.length} / {MAX_SKILLS} skills
        </span>
      </div>
      {error && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
