// @vitest-environment jsdom
// The Skills tab: list, open, edit, remove. Remove asks first. Titles carry the safety tags.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type MemoryView, type SkillRef } from '../../../lib/companion';
import { SkillsPanel } from './SkillsPanel';

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
  vi.restoreAllMocks();
});

const skill = (over: Partial<SkillRef> = {}): SkillRef => ({
  slug: 'weekly-report',
  title: 'Weekly report',
  chars: 120,
  unreviewed: false,
  flagged: null,
  ...over,
});

function seeded(skills: SkillRef[]) {
  return fakeCompanionTransport({
    memory: { skills },
    skillBodies: { 'weekly-report': 'Start with wins.\nThen blockers.', 'standup-notes': 'Three lines.' },
  });
}

async function render(skills: SkillRef[], transport = seeded(skills), onChanged: (m: MemoryView) => void = () => {}) {
  await act(async () =>
    root.render(<SkillsPanel companionId="mochi" name="Mochi" skills={skills} transport={transport} onChanged={onChanged} />),
  );
  return transport;
}

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;
const rowButton = (title: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('.ck-skills-open')].find((b) => b.textContent?.includes(title))!;

function type(selector: string, text: string) {
  const el = host.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector)!;
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, text);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

describe('SkillsPanel', () => {
  it('says what skills are when there are none', async () => {
    await render([]);
    expect(host.textContent).toContain('No skills yet. When Mochi learns a way you like things done, it will ask you here.');
    expect(host.textContent).toContain('0 / 30 skills');
  });

  it('lists each skill with its size and the safety tags', async () => {
    await render([skill(), skill({ slug: 'standup-notes', title: 'Standup notes', chars: 1200, unreviewed: true, flagged: 'Has a link' })]);
    const rows = [...host.querySelectorAll('.ck-skills-row')];
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('Weekly report');
    expect(rows[0].textContent).toContain('120 characters');
    expect(rows[1].textContent).toContain('1,200 characters');
    expect([...rows[1].querySelectorAll('.ck-memory-tag')].map((t) => t.textContent)).toEqual([
      'Changed outside Puppet Master',
      'Has a link',
    ]);
    expect(host.textContent).toContain('2 / 30 skills');
  });

  it('opens a skill to read its steps, and Back returns to the list', async () => {
    const transport = await render([skill()]);
    await act(async () => rowButton('Weekly report').click());
    expect(transport.calls.find((c) => c.method === 'getSkill')?.args).toEqual(['mochi', 'weekly-report']);
    expect(host.querySelector('.ck-skill-body')?.textContent).toBe('Start with wins.\nThen blockers.');
    act(() => button('Back').click());
    expect(host.querySelector('.ck-skills-list')).toBeTruthy();
  });

  it('edits and saves: Save is off for an empty title or steps over the cap', async () => {
    const onChanged = vi.fn();
    const transport = await render([skill()], undefined, onChanged);
    await act(async () => rowButton('Weekly report').click());
    act(() => button('Edit').click());
    type('textarea', 'x'.repeat(4100));
    expect(host.textContent).toContain('4,118 / 4,096');
    expect(button('Save').disabled).toBe(true);
    type('textarea', 'Only the wins.');
    type('input', '  ');
    expect(button('Save').disabled).toBe(true);
    type('input', 'Friday report');
    expect(button('Save').disabled).toBe(false);
    await act(async () => button('Save').click());
    expect(transport.calls.find((c) => c.method === 'saveSkill')?.args).toEqual([
      'mochi',
      'weekly-report',
      'Friday report',
      'Only the wins.',
    ]);
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(host.querySelector('.ck-skills-list')).toBeTruthy();
  });

  it('asks before it removes, and No keeps the skill', async () => {
    const transport = await render([skill()]);
    await act(async () => rowButton('Weekly report').click());
    act(() => button('Remove').click());
    expect(host.textContent).toContain('Remove it?');
    act(() => button('No').click());
    expect(transport.calls.some((c) => c.method === 'removeSkill')).toBe(false);
    act(() => button('Remove').click());
    await act(async () => button('Yes, remove').click());
    expect(transport.calls.find((c) => c.method === 'removeSkill')?.args).toEqual(['mochi', 'weekly-report']);
  });

  it('a skill that changed outside the app can be kept as it is', async () => {
    const transport = await render([skill({ unreviewed: true })]);
    await act(async () => rowButton('Weekly report').click());
    expect(host.querySelector('[role="status"]')?.textContent).toContain('Mochi will not list it until you keep it.');
    await act(async () => button('Keep it').click());
    expect(transport.calls.find((c) => c.method === 'saveSkill')?.args).toEqual([
      'mochi',
      'weekly-report',
      'Weekly report',
      'Start with wins.\nThen blockers.',
    ]);
  });

  it('shows a plain sentence when the command fails', async () => {
    const transport = await render([skill()]);
    transport.getSkill = vi.fn(async () => {
      throw new Error('No skill has that name.');
    });
    await act(async () => rowButton('Weekly report').click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('No skill has that name.');
  });
});
