// @vitest-environment jsdom
// Settings > Potato Bot > Sandbox: the status and the buttons that fit it, Reset behind a confirm, Remove behind a typed word, the
// idle-stop minutes, the confirm dialog for Windows' Linux feature, and the rows being hidden until Labs turns the sandbox on.
import { act } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fakeSandboxTransport, type FakeSandboxSeed, type FakeSandboxTransport, type SandboxView } from '../../../lib/potato-sandbox';
import { SECTIONS } from '../catalog';
import { visibleSections } from '../model';
import { blur, buttonByText, byLabel, click, hasText, mountRows, settle, type as typeInto, unsearchableLabels, type Mounted } from '../test-utils';
import { makeSandboxRows } from './SandboxRows';

let m: Mounted | undefined;
afterEach(async () => {
  await m?.unmount();
  m = undefined;
  vi.useRealTimers();
});

const INSTALLED: Partial<SandboxView> = { state: 'stopped', diskBytes: 3.7 * 1024 ** 3, hasResetPoint: true, resetPointBytes: 2.99 * 1024 ** 3 };

async function mount(seed: FakeSandboxSeed = {}, confirm: boolean = true) {
  const fake = fakeSandboxTransport({ detected: { languages: [], packageManager: null, projects: 0 }, ...seed });
  m = await mountRows(makeSandboxRows(fake).filter((r) => r.id !== 'potato-sandbox-labs'));
  m.env.confirm.mockImplementation(async () => confirm);
  await settle(6);
  return { fake, host: m.host, env: m.env };
}

const calls = (fake: FakeSandboxTransport, method: string) => fake.calls.filter((c) => c.method === method);
const state = (h: HTMLElement) => h.querySelector('.sbx-state')?.textContent;

describe('the status', () => {
  it('a sandbox that is not set up offers Set up and nothing else', async () => {
    const { host: h } = await mount();
    expect(state(h)).toBe('Not set up');
    expect(buttonByText(h, 'Set up…')!.disabled).toBe(false);
    for (const word of ['Start', 'Stop', 'Reset…', 'Remove…']) expect(buttonByText(h, word), word).toBeUndefined();
    expect(hasText(h, 'It cannot see your files, programs or accounts.')).toBe(true);
  });

  it('a stopped sandbox shows its disk and reset point and offers Start, Reset and Remove', async () => {
    const { host: h } = await mount({ status: INSTALLED });
    expect(state(h)).toBe('Stopped');
    expect(hasText(h, 'Uses 3.7 GB on this computer')).toBe(true);
    expect(hasText(h, 'Reset point: 3.0 GB')).toBe(true);
    expect(buttonByText(h, 'Start')).toBeTruthy();
    expect(buttonByText(h, 'Reset…')!.disabled).toBe(false);
    expect(buttonByText(h, 'Remove…')).toBeTruthy();
    expect(buttonByText(h, 'Set up…')).toBeUndefined();
  });

  it('a running sandbox offers Stop; a failed one says why and offers Start again', async () => {
    const run = await mount({ status: { ...INSTALLED, state: 'running' } });
    expect(state(run.host)).toBe('Running');
    expect(buttonByText(run.host, 'Stop')).toBeTruthy();
    expect(buttonByText(run.host, 'Start')).toBeUndefined();
    await m!.unmount();
    const bad = await mount({ status: { ...INSTALLED, state: 'failed', reason: 'The sandbox walls did not pass their check.' } });
    expect(state(bad.host)).toBe('Could not start. The sandbox walls did not pass their check.');
    expect(buttonByText(bad.host, 'Start again')).toBeTruthy();
  });

  it('Reset is off when no reset point was saved', async () => {
    const { host: h } = await mount({ status: { ...INSTALLED, hasResetPoint: false, resetPointBytes: null } });
    expect(buttonByText(h, 'Reset…')!.disabled).toBe(true);
  });

  it('Start and Stop call the app and the status follows', async () => {
    const { fake, host: h } = await mount({ status: INSTALLED });
    await click(buttonByText(h, 'Start'));
    await settle(4);
    expect(calls(fake, 'start')).toHaveLength(1);
    expect(state(h)).toBe('Running');
    await click(buttonByText(h, 'Stop'));
    await settle(4);
    expect(calls(fake, 'stop')).toHaveLength(1);
    expect(state(h)).toBe('Stopped');
  });

  it('an error from the app is shown as its sentence, without the code', async () => {
    const { fake, host: h } = await mount({ status: INSTALLED });
    fake.start = () => Promise.reject(new Error('SANDBOX_BAD_START: The sandbox did not answer in time.'));
    await click(buttonByText(h, 'Start'));
    await settle(4);
    expect(h.querySelector('[role="alert"]')?.textContent).toBe('The sandbox did not answer in time.');
  });

  it('reads the status again when the app says something changed, and every 15 seconds', async () => {
    vi.useFakeTimers();
    const { fake, host: h } = await mount({ status: INSTALLED });
    expect(state(h)).toBe('Stopped');
    await act(async () => fake.patch({ state: 'running' }));
    await settle(4);
    expect(state(h)).toBe('Running');
    // the app stopped it by itself (idle): no event, the poll finds it
    fake.view.state = 'stopped';
    const before = calls(fake, 'status').length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(15_100);
    });
    await settle(4);
    expect(calls(fake, 'status').length).toBeGreaterThan(before);
    expect(state(h)).toBe('Stopped');
  });
});

describe("Setup didn't finish", () => {
  const CUT_OFF: Partial<SandboxView> = { state: 'incomplete', reason: "Setup didn't finish.", canContinue: true };

  it('offers Continue setup and Start over, and never Start', async () => {
    const { host: h } = await mount({ status: CUT_OFF });
    expect(state(h)).toBe("Setup didn't finish");
    expect(buttonByText(h, 'Continue setup')).toBeTruthy();
    expect(buttonByText(h, 'Start over…')).toBeTruthy();
    for (const word of ['Start', 'Start again', 'Reset…', 'Set up…']) expect(buttonByText(h, word), word).toBeUndefined();
  });

  it('Continue setup carries on and the page shows the bar again', async () => {
    const { fake, host: h } = await mount({ status: CUT_OFF });
    await click(buttonByText(h, 'Continue setup'));
    await settle(4);
    expect(calls(fake, 'continueInstall')).toHaveLength(1);
    expect(state(h)).toBe('Setting up');
    expect(buttonByText(h, 'Stop set up')).toBeTruthy();
  });

  it('without known answers only Start over is offered', async () => {
    const { host: h } = await mount({ status: { ...CUT_OFF, canContinue: false } });
    expect(buttonByText(h, 'Continue setup')).toBeUndefined();
    expect(buttonByText(h, 'Start over…')).toBeTruthy();
  });

  it('Start over asks for the word, removes, and opens the wizard', async () => {
    const { fake, host: h } = await mount({ status: CUT_OFF });
    await click(buttonByText(h, 'Start over…'));
    await settle(2);
    const typed = byLabel(h, 'Type remove to confirm') as HTMLInputElement;
    await typeInto(typed, 'remove');
    await click(buttonByText(h, 'Remove sandbox'));
    await settle(6);
    expect(calls(fake, 'remove')).toHaveLength(1);
    expect(state(h)).toBe('Not set up');
    expect(document.querySelector('[role="dialog"]')).toBeTruthy();
  });
});

describe('Set up', () => {
  it('opens the wizard, and opens nothing until pressed', async () => {
    const { host: h } = await mount();
    expect(h.querySelector('[role="dialog"]')).toBeNull();
    await click(buttonByText(h, 'Set up…'));
    await settle(6);
    // The wizard is a modal in a portal at the document root, not inside the section card.
    const dialog = document.body.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain('Set up the sandbox');
    expect(h.contains(dialog)).toBe(false);
  });

  it('shows the bar and Stop set up while it runs, even with the wizard closed', async () => {
    const { fake, host: h } = await mount();
    await act(async () => fake.pushInstall({ percent: 33, step: 'create', title: 'Downloading Ubuntu', index: 3, of: 8 }));
    await settle(4);
    expect(state(h)).toBe('Setting up');
    expect(h.querySelector('[role="progressbar"]')!.getAttribute('aria-valuenow')).toBe('33');
    expect(h.querySelector('.sbx-step-text')?.textContent).toBe('Downloading Ubuntu');
    expect(buttonByText(h, 'Set up…')).toBeUndefined();
    await click(buttonByText(h, 'Stop set up'));
    await settle(4);
    expect(calls(fake, 'cancelInstall')).toHaveLength(1);
  });
});

describe('Reset', () => {
  it('asks first, and resets only after the person says yes', async () => {
    const no = await mount({ status: INSTALLED }, false);
    await click(buttonByText(no.host, 'Reset…'));
    await settle(4);
    expect(no.env.confirm).toHaveBeenCalledTimes(1);
    expect(no.env.confirm.mock.calls[0][0]).toMatchObject({ title: 'Reset the sandbox?', confirmLabel: 'Reset' });
    expect(no.env.confirm.mock.calls[0][0].body).toContain('Everything done in it since is lost');
    expect(calls(no.fake, 'reset')).toHaveLength(0);
    await m!.unmount();
    const yes = await mount({ status: { ...INSTALLED, state: 'running' } }, true);
    await click(buttonByText(yes.host, 'Reset…'));
    await settle(4);
    expect(calls(yes.fake, 'reset')).toHaveLength(1);
    expect(hasText(yes.host, 'The sandbox is back to its reset point.')).toBe(true);
    expect(state(yes.host)).toBe('Stopped');
  });
});

describe('Remove', () => {
  it('needs the word remove typed; the button stays off until then', async () => {
    const { fake, host: h } = await mount({ status: INSTALLED });
    await click(buttonByText(h, 'Remove…'));
    expect(h.querySelector('[role="dialog"]')?.textContent).toContain('Remove the sandbox?');
    const field = byLabel<HTMLInputElement>(h, 'Type remove to confirm')!;
    const remove = () => buttonByText(h, 'Remove sandbox')!;
    expect(remove().disabled).toBe(true);
    await typeInto(field, 'delete');
    expect(remove().disabled).toBe(true);
    await typeInto(field, 'remov');
    expect(remove().disabled).toBe(true);
    await click(remove());
    expect(calls(fake, 'remove')).toHaveLength(0);
    await typeInto(field, ' Remove ');
    expect(remove().disabled).toBe(false);
    await click(remove());
    await settle(4);
    expect(calls(fake, 'remove').map((c) => c.args)).toEqual([['remove']]);
    expect(h.querySelector('[role="dialog"]')).toBeNull();
    expect(state(h)).toBe('Not set up');
    expect(hasText(h, 'The sandbox was removed.')).toBe(true);
  });

  it('Cancel removes nothing', async () => {
    const { fake, host: h } = await mount({ status: INSTALLED });
    await click(buttonByText(h, 'Remove…'));
    await click(buttonByText(h, 'Cancel'));
    expect(h.querySelector('[role="dialog"]')).toBeNull();
    expect(calls(fake, 'remove')).toHaveLength(0);
  });
});

describe("Windows' Linux feature", () => {
  it('a PC without it says so, and the button asks before anything runs', async () => {
    const { fake, host: h } = await mount({ status: { linuxFeature: false } });
    expect(hasText(h, "Needs Windows' Linux feature (admin + restart).")).toBe(true);
    expect(buttonByText(h, 'Set up…')!.disabled).toBe(true);
    await click(buttonByText(h, "Turn on Windows' Linux feature…"));
    await settle(4);
    expect(h.querySelector('[role="dialog"]')?.textContent).toContain("Turn on Windows' Linux feature?");
    expect(calls(fake, 'linuxEnable')).toHaveLength(0);
    await click(buttonByText(h, 'Cancel'));
    await settle(2);
    expect(calls(fake, 'linuxEnable')).toHaveLength(0);
    expect(h.querySelector('[role="dialog"]')).toBeNull();
  });

  it('the dialog button turns it on with the dialog token; then Set up is allowed and a restart is mentioned', async () => {
    const { fake, host: h } = await mount({ status: { linuxFeature: false } });
    await click(buttonByText(h, "Turn on Windows' Linux feature…"));
    await settle(4);
    await click(buttonByText(h, 'Turn on'));
    await settle(6);
    expect(calls(fake, 'linuxEnable').map((c) => c.args)).toEqual([['fake-token']]);
    expect(hasText(h, 'Restart the computer, then set up the sandbox.')).toBe(true);
    expect(buttonByText(h, 'Set up…')!.disabled).toBe(false);
  });
});

describe('Stop when idle', () => {
  it('shows the saved minutes and saves a new number kept between 1 and 240', async () => {
    const { fake, host: h, env } = await mount({ status: { ...INSTALLED, idleMinutes: 20 } });
    const field = byLabel<HTMLInputElement>(h, 'Stop the sandbox after minutes idle')!;
    expect(field.value).toBe('20');
    await typeInto(field, '45');
    await blur(field);
    await settle(4);
    expect(calls(fake, 'setIdle').map((c) => c.args)).toEqual([[45]]);
    expect(env.markSaved).toHaveBeenCalled();
    await typeInto(field, '9000');
    await blur(field);
    await settle(4);
    expect(calls(fake, 'setIdle').pop()!.args).toEqual([240]);
    expect(field.value).toBe('240');
  });
});

describe('where the rows show', () => {
  it('only while Labs has the sandbox on (and Potato Bot)', () => {
    const rowIds = (settings: { potato_bot_enabled?: boolean; potato_sandbox_enabled?: boolean } | null) =>
      (visibleSections(SECTIONS, settings).find((s) => s.id === 'potato')?.rows ?? []).map((r) => r.id);
    expect(rowIds(null)).toEqual([]);
    const base = ['potato-bot', 'potato-models', 'potato-key', 'potato-persona', 'potato-limit', 'potato-usage', 'potato-voice', 'potato-voice-rate', 'potato-voice-test'];
    expect(rowIds({ potato_bot_enabled: true })).toEqual(base);
    expect(rowIds({ potato_bot_enabled: true, potato_sandbox_enabled: false })).not.toContain('potato-sandbox');
    expect(rowIds({ potato_bot_enabled: true, potato_sandbox_enabled: true })).toEqual([
      ...base, 'potato-sandbox', 'potato-sandbox-idle',
    ]);
    // the sandbox switch alone does not show a section that is off
    expect(rowIds({ potato_sandbox_enabled: true })).toEqual([]);
  });

  it('the switch lives in General > Labs, where a person turns it on, and turns it on with one patch', async () => {
    const general = SECTIONS.find((s) => s.id === 'general')!;
    const labs = general.rows.find((r) => r.id === 'potato-sandbox-labs');
    expect(labs?.labs?.name).toBe('Sandbox');
    expect(labs?.group).toBe('Labs');
    expect(general.rows.find((r) => r.id === 'potato-sandbox')).toBeUndefined();
    m = await mountRows([labs!]);
    await settle(3);
    await click(m.host.querySelector('[role="switch"]'));
    await settle(2);
    expect(m.env.patches).toEqual([{ potato_sandbox_enabled: true }]);
  });

  it('every word of every label finds its row in search', async () => {
    const fake = fakeSandboxTransport({ status: INSTALLED });
    const rows = makeSandboxRows(fake);
    m = await mountRows(rows);
    await settle(6);
    expect(unsearchableLabels(m.host, rows)).toEqual([]);
  });
});
