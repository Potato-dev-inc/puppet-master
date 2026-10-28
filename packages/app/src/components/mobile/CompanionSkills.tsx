// The skills a companion has learned, on the phone (read only). Titles are all the phone gets: the steps stay on the
// computer, where the person approves, edits and removes skills (SPEC-1028, companion skills).
import type { SkillRef } from '../../lib/companion';

export function CompanionSkills({ name, skills }: { name: string; skills: SkillRef[] }) {
  if (skills.length === 0) return null;
  return (
    <section className="ph-comp-block" aria-label={`What ${name} can do`}>
      <h2 className="ph-comp-title">Skills {name} has learned</h2>
      <ul className="ph-group">
        {skills.map((skill) => (
          <li key={skill.slug} className="ph-comp-pending">
            <p className="ph-comp-text">{skill.title}</p>
            {skill.unreviewed && <p className="ph-comp-flag">Changed outside Puppet Master</p>}
          </li>
        ))}
      </ul>
    </section>
  );
}
