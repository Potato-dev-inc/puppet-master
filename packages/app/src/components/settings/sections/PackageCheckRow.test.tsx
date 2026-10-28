// @vitest-environment jsdom
// Settings > General > Safety: "Check packages before installs". It loads the saved switch, saves each change
// through packageCheckSettingsSet, shows the off note, and a failed save puts the switch back with a plain error.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LAYOUT } from '../arrange';
import { generalRows } from './GeneralSection';
import { PACKAGE_CHECK_LABEL, PACKAGE_CHECK_NOTE, PACKAGE_CHECK_OFF_NOTE, PackageCheckRow, packageCheckSaveError } from './PackageCheckRow';
import { packageCheckSettingsGet, packageCheckSettingsSet } from '../../../lib/package-check';
import { rowMatches } from '../model';
import { byLabel, click, hasText, mountRows, settle, type Mounted } from '../test-utils';

vi.mock('../../../lib/package-check', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/package-check')>();
  return { ...real, packageCheckSettingsGet: vi.fn(), packageCheckSettingsSet: vi.fn() };
});

let m: Mounted;

async function mount(saved: { lookups: boolean } | Error = { lookups: true }) {
  if (saved instanceof Error) vi.mocked(packageCheckSettingsGet).mockRejectedValue(saved);
  else vi.mocked(packageCheckSettingsGet).mockResolvedValue(saved);
  vi.mocked(packageCheckSettingsSet).mockImplementation(async (next) => next);
  m = await mountRows([{ id: 'package-check', label: PACKAGE_CHECK_LABEL, group: 'Safety', render: (env) => <PackageCheckRow onSaved={env.markSaved} /> }]);
  await settle(10);
  return m;
}

const switchEl = () => byLabel<HTMLButtonElement>(m.host, PACKAGE_CHECK_LABEL);

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(async () => {
  await m?.unmount();
});

describe('Check packages before installs: loading and the note', () => {
  it('loads the saved value: on, with the note and no off note', async () => {
    await mount({ lookups: true });
    expect(switchEl()!.getAttribute('aria-checked')).toBe('true');
    expect(hasText(m.host, PACKAGE_CHECK_NOTE)).toBe(true);
    expect(hasText(m.host, 'Only the package name is sent.')).toBe(true);
    expect(hasText(m.host, PACKAGE_CHECK_OFF_NOTE)).toBe(false);
  });

  it('shows the off note when the saved value is off', async () => {
    await mount({ lookups: false });
    expect(switchEl()!.getAttribute('aria-checked')).toBe('false');
    expect(hasText(m.host, PACKAGE_CHECK_OFF_NOTE)).toBe(true);
  });

  it('says it needs the desktop app when the setting cannot be read here', async () => {
    await mount(new Error('not available'));
    expect(hasText(m.host, 'Available in the desktop app.')).toBe(true);
    expect(switchEl()).toBeNull();
  });
});

describe('Check packages before installs: saving', () => {
  it('a toggle saves the new value, ticks the row, and shows the off note', async () => {
    await mount({ lookups: true });
    await click(switchEl());
    await settle();
    expect(packageCheckSettingsSet).toHaveBeenCalledExactlyOnceWith({ lookups: false });
    expect(m.env.markSaved).toHaveBeenCalledTimes(1);
    expect(switchEl()!.getAttribute('aria-checked')).toBe('false');
    expect(hasText(m.host, PACKAGE_CHECK_OFF_NOTE)).toBe(true);
    expect(m.env.patches).toEqual([]); // not a settings.json patch
  });

  it('turning it back on saves lookups: true', async () => {
    await mount({ lookups: false });
    await click(switchEl());
    await settle();
    expect(packageCheckSettingsSet).toHaveBeenCalledExactlyOnceWith({ lookups: true });
    expect(switchEl()!.getAttribute('aria-checked')).toBe('true');
  });

  it('a failed save puts the switch back and shows a plain error, without the code', async () => {
    await mount({ lookups: true });
    vi.mocked(packageCheckSettingsSet).mockRejectedValue(new Error('SETTINGS_WRITE: The setting could not be saved.'));
    await click(switchEl());
    await settle();
    expect(switchEl()!.getAttribute('aria-checked')).toBe('true');
    const alert = m.host.querySelector('[role="alert"]');
    expect(alert?.textContent).toBe('The setting could not be saved.');
    expect(m.env.markSaved).not.toHaveBeenCalled();
  });
});

describe('packageCheckSaveError', () => {
  it('drops the CODE: prefix and keeps the sentence', () => {
    expect(packageCheckSaveError(new Error('SETTINGS_WRITE: Disk is full.'))).toBe('Disk is full.');
    expect(packageCheckSaveError('Plain text.')).toBe('Plain text.');
  });

  it('falls back to a plain sentence when there is no text', () => {
    expect(packageCheckSaveError(new Error(''))).toBe("Couldn't save. Try again.");
    expect(packageCheckSaveError('CODE: ')).toBe("Couldn't save. Try again.");
  });
});

describe('Check packages before installs: where it lives and how it is found', () => {
  it('is a General row laid out in Safety, right after Warn when an agent seems stuck', () => {
    const safety = LAYOUT.general!.find((g) => g.group === 'Safety')!.rows;
    expect(safety.indexOf('package-check')).toBe(safety.indexOf('loop-guard') + 1);
    expect(generalRows.find((r) => r.id === 'package-check')?.label).toBe(PACKAGE_CHECK_LABEL);
  });

  it('is found by a word of its note and by the registry names', () => {
    const row = generalRows.find((r) => r.id === 'package-check')!;
    expect(rowMatches(row, 'packages')).toBe(true);
    expect(rowMatches(row, 'pypi')).toBe(true);
    expect(rowMatches(row, 'npm')).toBe(true);
  });
});
