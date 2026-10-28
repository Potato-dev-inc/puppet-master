// @vitest-environment jsdom
// A proposed skill in "{name} wants to remember": Approve / Edit / Reject, the same safety tags as a note.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport, type PendingItem } from '../../../lib/companion';
import { PendingCard } from './PendingCard';

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

const proposal = (over: Partial<PendingItem> = {}): PendingItem => ({
  id: 's1s2s3s4s5s6',
  kind: 'skill',
  text: 'Weekly report',
  body: 'Start with wins.\nThen blockers.',
  flagged: null,
  targetLine: null,
  ...over,
});

const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;

function type(selector: string, text: string) {
  const el = host.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector)!;
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, text);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function render(items: PendingItem[], transport = fakeCompanionTransport({ memory: { pending: items } }), onChanged = vi.fn()) {
  await act(async () =>
    root.render(<PendingCard companionId="mochi" name="Mochi" items={items} transport={transport} onChanged={onChanged} />),
  );
  return { transport, onChanged };
}

describe('a proposed skill', () => {
  it('shows the title, the steps and a New skill label, with Approve, Edit and Reject', async () => {
    await render([proposal()]);
    expect(host.querySelector('h2')?.textContent).toBe('Mochi wants to remember');
    expect(host.querySelector('.ck-skill-kind')?.textContent).toBe('New skill');
    expect(host.querySelector('.ck-skill-title')?.textContent).toBe('Weekly report');
    expect(host.querySelector('.ck-skill-body')?.textContent).toBe('Start with wins.\nThen blockers.');
    expect(['Approve', 'Edit', 'Reject'].every((l) => button(l))).toBe(true);
    expect(button('Keep')).toBeUndefined();
  });

  it('shows the same flags as a note and does not block Approve', async () => {
    await render([proposal({ flagged: 'Sounds like an instruction' })]);
    expect(host.querySelector('.ck-memory-tag')?.textContent).toBe('Sounds like an instruction');
    expect(button('Approve').disabled).toBe(false);
  });

  it('Approve keeps it as proposed', async () => {
    const { transport, onChanged } = await render([proposal()]);
    await act(async () => button('Approve').click());
    expect(transport.calls.find((c) => c.method === 'decidePending')?.args).toEqual(['mochi', 's1s2s3s4s5s6', 'keep']);
    expect(transport.calls.some((c) => c.method === 'approveSkill')).toBe(false);
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it('Reject drops it', async () => {
    const { transport } = await render([proposal()]);
    await act(async () => button('Reject').click());
    expect(transport.calls.find((c) => c.method === 'decidePending')?.args).toEqual(['mochi', 's1s2s3s4s5s6', 'drop']);
  });

  it('Edit lets the person change the title and steps, then Approve sends the edit', async () => {
    const { transport, onChanged } = await render([proposal()]);
    act(() => button('Edit').click());
    expect((host.querySelector('input') as HTMLInputElement).value).toBe('Weekly report');
    type('input', 'Friday report');
    type('textarea', 'Only wins.');
    expect(host.textContent).toContain('/ 4,096');
    await act(async () => button('Approve').click());
    expect(transport.calls.find((c) => c.method === 'approveSkill')?.args).toEqual([
      'mochi',
      's1s2s3s4s5s6',
      { title: 'Friday report', body: 'Only wins.' },
    ]);
    expect(onChanged.mock.calls[0][0].skills[0].title).toBe('Friday report');
  });

  it('Cancel in the editor puts the proposal back as it was', async () => {
    await render([proposal()]);
    act(() => button('Edit').click());
    type('input', 'Something else');
    act(() => button('Cancel').click());
    expect(host.querySelector('.ck-skill-title')?.textContent).toBe('Weekly report');
  });

  it('Approve is off for empty steps and for a skill over 4,096 characters', async () => {
    await render([proposal()]);
    act(() => button('Edit').click());
    type('textarea', ' ');
    expect(button('Approve').disabled).toBe(true);
    type('textarea', 'x'.repeat(4100));
    expect(button('Approve').disabled).toBe(true);
    expect(host.querySelector('.ck-memory-err')?.textContent).toContain('/ 4,096');
  });

  it('a proposal that is already too long cannot be approved as it is, only edited', async () => {
    await render([proposal({ body: 'x'.repeat(4200) })]);
    expect(host.textContent).toContain('Too long for a skill. Edit it to make it shorter.');
    expect(button('Approve').disabled).toBe(true);
    expect(button('Edit').disabled).toBe(false);
  });

  it("notes and skills share the list: a note still has Keep and Don't keep", async () => {
    await render([proposal(), { id: 'n1', kind: 'note', text: 'Likes tea.', flagged: null, targetLine: null }]);
    expect(button('Keep')).toBeTruthy();
    expect(button("Don't keep")).toBeTruthy();
    expect(button('Approve')).toBeTruthy();
  });

  it('shows a plain sentence when the PC refuses (full or too long)', async () => {
    const transport = fakeCompanionTransport({ memory: { pending: [proposal()] } });
    transport.decidePending = vi.fn(async () => {
      throw new Error('You can have at most 30 skills. Remove one first.');
    });
    await render([proposal()], transport);
    await act(async () => button('Approve').click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('You can have at most 30 skills. Remove one first.');
  });
});
