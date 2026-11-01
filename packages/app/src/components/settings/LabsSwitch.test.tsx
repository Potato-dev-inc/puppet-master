// @vitest-environment jsdom
// Development build: every Labs switch reads as on and is disabled, and the Labs page says why.
// Outside a development build the switches are the plain ones.
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { RowDef, SectionDef } from './model';
import { arrangeSections, LABS_INDEX_ID } from './arrange';
import { LabsSwitch } from './LabsSwitch';
import { click, hasText, mountRows, settle, type Mounted } from './test-utils';
import { DEV_LABS_NOTE, setDevLabsOverrideForTests } from '../../lib/dev-labs';

let m: Mounted | undefined;
afterEach(async () => {
  await m?.unmount();
  m = undefined;
  setDevLabsOverrideForTests(false);
});

const switchFor = (label: string) => m?.host.querySelector<HTMLButtonElement>(`[role="switch"][aria-label="${label}"]`);

function switchRow(checked: boolean, onChange: (value: boolean) => void): RowDef {
  return {
    id: 'night-shift',
    label: 'Night Shift',
    render: () => <LabsSwitch label="Night Shift" description="Plan tasks for tonight." checked={checked} onChange={onChange} />,
  };
}

describe('a Labs switch', () => {
  it('in a development build reads as on, is disabled, and ignores a click', async () => {
    setDevLabsOverrideForTests(true);
    const onChange = vi.fn();
    m = await mountRows([switchRow(false, onChange)]);
    await settle(3);
    expect(switchFor('Night Shift')!.getAttribute('aria-checked')).toBe('true');
    expect(switchFor('Night Shift')!.disabled).toBe(true);
    await click(switchFor('Night Shift'));
    expect(onChange).not.toHaveBeenCalled();
  });

  it('elsewhere is the plain switch: it shows the stored value and saves a click', async () => {
    setDevLabsOverrideForTests(false);
    const onChange = vi.fn();
    m = await mountRows([switchRow(false, onChange)]);
    await settle(3);
    expect(switchFor('Night Shift')!.getAttribute('aria-checked')).toBe('false');
    expect(switchFor('Night Shift')!.disabled).toBe(false);
    await click(switchFor('Night Shift'));
    expect(onChange).toHaveBeenCalledWith(true);
  });
});

describe('the Labs page note', () => {
  const base: SectionDef[] = [
    {
      id: 'general',
      title: 'General',
      blurb: '',
      rows: [{ id: 'night-shift', label: 'Night Shift', labs: { name: 'Night Shift', about: 'Agents work overnight.' }, render: () => <span>switch</span> }],
    },
  ];
  const labsIndexRow = (): RowDef => arrangeSections(base).find((s) => s.id === 'general')!.rows.find((r) => r.id === LABS_INDEX_ID)!;

  it('is the first line of the Labs page while the development override is on', async () => {
    setDevLabsOverrideForTests(true);
    m = await mountRows([labsIndexRow()]);
    await settle(3);
    expect(hasText(m.host, DEV_LABS_NOTE)).toBe(true);
    const text = m.host.textContent ?? '';
    expect(text.indexOf(DEV_LABS_NOTE)).toBeLessThan(text.indexOf('Night Shift'));
  });

  it('is not shown in a build where the override is off', async () => {
    setDevLabsOverrideForTests(false);
    m = await mountRows([labsIndexRow()]);
    await settle(3);
    expect(hasText(m.host, DEV_LABS_NOTE)).toBe(false);
    expect(hasText(m.host, 'Night Shift')).toBe(true);
  });
});
