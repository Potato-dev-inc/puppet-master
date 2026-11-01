// The sandbox's estimate, the wizard's first draft (languages checked from the person's projects), the answers it sends, and the fake.
import { describe, expect, it } from 'vitest';
import {
  FAKE_MENU,
  REMOVE_WORD,
  answersOf,
  estimateFor,
  fakeSandboxTransport,
  formatGb,
  formatSize,
  initialDraft,
  joinWords,
  plainSandboxError,
  type WizardAnswers,
} from './potato-sandbox';

const base: WizardAnswers = { look: 'ubuntu', uses: ['coding'], options: { 'coding.pkg': 'npm' }, resetPoint: true };

describe('estimateFor', () => {
  it('starts from the measured base: the desktop and Chrome download about 0.7 GB and take 3.7 GB', () => {
    const light = estimateFor({ look: 'ubuntu', uses: [], options: {}, resetPoint: false });
    expect(light.downloadGb).toBe(0.7);
    expect(light.diskGb).toBe(3.7);
    expect(light.resetPointGb).toBe(0);
  });

  it('adds the Zorin look (83 MB, 393 MB, measured) and takes the light desktop off', () => {
    const plain = estimateFor({ ...base, uses: [] });
    const zorin = estimateFor({ ...base, uses: [], look: 'zorin' });
    expect(Math.round((zorin.diskGb - plain.diskGb) * 100) / 100).toBeCloseTo(0.4, 1);
    expect(zorin.downloadGb).toBeGreaterThanOrEqual(plain.downloadGb);
    expect(estimateFor({ ...base, uses: [], look: 'light' }).diskGb).toBeLessThan(plain.diskGb);
  });

  it('grows with coding, each language, extras, and daily use', () => {
    const none = estimateFor({ ...base, uses: [] });
    const coding = estimateFor(base);
    const rust = estimateFor({ ...base, options: { ...base.options, 'coding.languages': 'rust,go' } });
    const extras = estimateFor({ ...base, options: { ...base.options, 'coding.extras': 'tmux,ffmpeg' } });
    const daily = estimateFor({ look: 'ubuntu', uses: ['daily'], options: { 'daily.media': 'vlc', 'daily.images': 'eog' }, resetPoint: true });
    expect(coding.diskGb).toBeGreaterThan(none.diskGb);
    expect(rust.diskGb).toBeGreaterThan(coding.diskGb);
    expect(extras.diskGb).toBeGreaterThan(coding.diskGb);
    expect(daily.diskGb).toBeGreaterThan(none.diskGb);
    expect(estimateFor({ look: 'ubuntu', uses: ['daily'], options: { 'daily.media': 'none', 'daily.images': 'none' }, resetPoint: true }).diskGb).toBeLessThan(daily.diskGb);
  });

  it('the reset point is about 0.8 of the disk (2.99 GB for 3.7 GB, measured) and only when it is on', () => {
    const on = estimateFor(base);
    expect(on.resetPointGb).toBeCloseTo(on.diskGb * 0.8, 1);
    expect(estimateFor({ ...base, resetPoint: false }).resetPointGb).toBe(0);
  });

  it('gives a time range that widens with the work', () => {
    const [lo, hi] = estimateFor(base).minutes;
    expect(lo).toBeGreaterThanOrEqual(8);
    expect(hi).toBeGreaterThan(lo);
    expect(estimateFor({ ...base, uses: ['coding', 'daily'] }).minutes[0]).toBeGreaterThan(lo);
  });
});

describe('formatting', () => {
  it('sizes and words', () => {
    expect(formatSize(3.7 * 1024 ** 3)).toBe('3.7 GB');
    expect(formatSize(640 * 1024 ** 2)).toBe('640 MB');
    expect(formatSize(10)).toBe('1 MB');
    expect(formatGb(1.24)).toBe('1.2 GB');
    expect(formatGb(0.4)).toBe('400 MB');
    expect(joinWords([])).toBe('');
    expect(joinWords(['Python'])).toBe('Python');
    expect(joinWords(['Python', 'Go'])).toBe('Python and Go');
    expect(joinWords(['Python', 'Go', 'Rust'])).toBe('Python, Go and Rust');
    expect(plainSandboxError(new Error('SANDBOX_BUSY: Set up is still running.'), 'x')).toBe('Set up is still running.');
    expect(plainSandboxError(undefined, 'Fallback.')).toBe('Fallback.');
  });
});

describe('initialDraft', () => {
  it('starts with Ubuntu, coding, no languages and the recipe defaults when nothing was found in the projects', () => {
    const draft = initialDraft(FAKE_MENU, { languages: [], packageManager: null, projects: 4 });
    expect(draft).toMatchObject({ look: 'ubuntu', coding: true, daily: false, languages: [], packageManager: 'npm', extras: [], media: 'vlc', images: 'eog', resetPoint: true });
    expect(initialDraft(FAKE_MENU, null).languages).toEqual([]);
  });

  it('checks the languages the projects use and picks the package manager they use', () => {
    const draft = initialDraft(FAKE_MENU, { languages: ['rust', 'python', 'cobol'], packageManager: 'pnpm', projects: 3 });
    expect(draft.languages).toEqual(['rust', 'python']);
    expect(draft.packageManager).toBe('pnpm');
    expect(initialDraft(FAKE_MENU, { languages: [], packageManager: 'deno', projects: 1 }).packageManager).toBe('npm');
  });
});

describe('answersOf', () => {
  it('sends only the choices of the uses that are picked', () => {
    const draft = { ...initialDraft(FAKE_MENU, { languages: ['go', 'rust'], packageManager: 'bun', projects: 1 }), look: 'zorin' as const, extras: ['tmux'], daily: true };
    expect(answersOf(draft)).toEqual({
      look: 'zorin',
      uses: ['coding', 'daily'],
      options: { 'coding.pkg': 'bun', 'coding.languages': 'go,rust', 'coding.extras': 'tmux', 'daily.media': 'vlc', 'daily.images': 'eog' },
      resetPoint: true,
    });
    expect(answersOf({ ...draft, coding: false })).toEqual({ look: 'zorin', uses: ['daily'], options: { 'daily.media': 'vlc', 'daily.images': 'eog' }, resetPoint: true });
    expect(answersOf({ ...draft, daily: false, languages: [], extras: [], resetPoint: false })).toEqual({ look: 'zorin', uses: ['coding'], options: { 'coding.pkg': 'bun' }, resetPoint: false });
  });
});

describe('fakeSandboxTransport', () => {
  it('follows the real one: install runs until the app says it is done, remove needs the word', async () => {
    const fake = fakeSandboxTransport();
    const seen: string[] = [];
    await fake.onInstall!((v) => seen.push(`${v.state}:${v.percent}`));
    expect((await fake.status()).state).toBe('not_installed');
    await fake.install(base);
    fake.pushInstall({ percent: 40, title: 'Downloading Ubuntu' });
    fake.finishInstall();
    expect(seen).toEqual(['running:0', 'running:40', 'done:100']);
    expect((await fake.status()).state).toBe('stopped');
    await expect(fake.remove('delete')).rejects.toThrow(/Type the word remove/);
    await fake.remove(` ${REMOVE_WORD.toUpperCase()} `);
    expect((await fake.status()).state).toBe('not_installed');
    expect(fake.calls.map((c) => c.method)).toEqual(['status', 'install', 'status', 'remove', 'remove', 'status']);
  });

  it('refuses an install without the Linux feature, and the feature needs the dialog token', async () => {
    const fake = fakeSandboxTransport({ status: { linuxFeature: false } });
    await expect(fake.install(base)).rejects.toThrow(/SANDBOX_NEEDS_LINUX_FEATURE/);
    await expect(fake.linuxEnable('guess')).rejects.toThrow(/SANDBOX_NOT_CONFIRMED/);
    const ask = await fake.linuxAsk();
    expect(await fake.linuxEnable(ask.token)).toEqual({ restartNeeded: true });
    expect(fake.view.linuxFeature).toBe(true);
  });

  it('keeps the idle minutes between 1 and 240', async () => {
    const fake = fakeSandboxTransport();
    expect(await fake.setIdle(0)).toBe(1);
    expect(await fake.setIdle(9999)).toBe(240);
    expect(await fake.setIdle(30)).toBe(30);
  });
});
