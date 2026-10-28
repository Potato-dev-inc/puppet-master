// @vitest-environment jsdom
// The phone lists the skill titles and nothing else.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { SkillRef } from '../../lib/companion';
import { CompanionSkills } from './CompanionSkills';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const skills: SkillRef[] = [
  { slug: 'weekly-report', title: 'Weekly report', chars: 90, unreviewed: false, flagged: null },
  { slug: 'standup', title: 'Standup', chars: 40, unreviewed: true, flagged: null },
];

describe('CompanionSkills', () => {
  it('lists the titles, read only, with a note on one that changed outside the app', () => {
    act(() => root.render(<CompanionSkills name="Mochi" skills={skills} />));
    expect(host.querySelector('h2')?.textContent).toBe('Skills Mochi has learned');
    expect([...host.querySelectorAll('li p:first-child')].map((p) => p.textContent)).toEqual(['Weekly report', 'Standup']);
    expect(host.textContent).toContain('Changed outside Puppet Master');
    expect(host.querySelectorAll('button, input, textarea')).toHaveLength(0);
  });

  it('draws nothing when there are none', () => {
    act(() => root.render(<CompanionSkills name="Mochi" skills={[]} />));
    expect(host.textContent).toBe('');
  });
});
