// Pure helpers of the crew sheets: editing a draft, what blocks Save, and the sentence for a role that failed to start.
import type { Crew, CrewRole, CrewStartOutcome } from '../../../lib/crews-api';
import { explainError } from '../error-sentences';

export const MAX_ROLE_NAME = 40;
export const MAX_CREW_NAME = 60;

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** Rename a role; the roles under it follow the new name. */
export function renameRole(crew: Crew, index: number, name: string): Crew {
  const old = crew.roles[index]?.name;
  if (old === undefined) return crew;
  return {
    ...crew,
    roles: crew.roles.map((r, i) => (i === index ? { ...r, name } : r.parent && same(r.parent, old) ? { ...r, parent: name } : r)),
  };
}

export function setTask(crew: Crew, index: number, task: string): Crew {
  return { ...crew, roles: crew.roles.map((r, i) => (i === index ? { ...r, task } : r)) };
}

/** Take a helper out of the draft. The roles under it move up to its own parent. The lead stays. */
export function dropRole(crew: Crew, index: number): Crew {
  const gone = crew.roles[index];
  if (!gone || !gone.parent) return crew;
  return {
    ...crew,
    roles: crew.roles
      .filter((_, i) => i !== index)
      .map((r) => (r.parent && same(r.parent, gone.name) ? { ...r, parent: gone.parent } : r)),
  };
}

/** What stops Save before the server is asked: an empty name or task, or two roles with one name. */
export function blockedReason(crew: Crew): string | null {
  if (!crew.name.trim()) return 'Give the crew a name.';
  for (const r of crew.roles) {
    if (!r.name.trim()) return 'Every role needs a name.';
    if (!r.task.trim()) return `Write a task for ${r.name.trim()}.`;
  }
  const names = crew.roles.map((r) => r.name.trim().toLowerCase());
  if (new Set(names).size !== names.length) return 'Two roles have the same name.';
  return null;
}

/** "under Lead" or "lead". */
export function roleLine(role: CrewRole): string {
  return role.parent ? `under ${role.parent}` : 'lead';
}

/** The words for a role that did not start: the known sentence for its code (named by the role), else the message. */
export function failureText(outcome: CrewStartOutcome): string | null {
  const f = outcome.failed;
  if (!f) return null;
  const explained = explainError(f.code, f.role);
  return explained ? `${explained.title}. ${explained.sentence}` : f.message;
}
