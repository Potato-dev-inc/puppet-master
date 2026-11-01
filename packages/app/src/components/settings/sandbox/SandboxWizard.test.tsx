// @vitest-environment jsdom
// The sandbox setup wizard: the steps and their order, the languages pre-checked from the person's projects, the summary with its
// estimate, Install (nothing installs before it), the progress bar and its end screens, and the confirm dialog for Windows' Linux feature.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { emptyInstall, fakeSandboxTransport, type FakeSandboxSeed, type FakeSandboxTransport } from '../../../lib/potato-sandbox';
import { buttonByText, click, hasText, settle } from '../test-utils';
import { SandboxWizard } from './SandboxWizard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;
let host: HTMLDivElement | undefined;

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = undefined;
  host = undefined;
});

const DETECTED = { languages: ['rust', 'python'], packageManager: 'pnpm', projects: 3 };

async function mount(seed: FakeSandboxSeed = {}) {
  const fake = fakeSandboxTransport({ detected: DETECTED, ...seed });
  const onClose = vi.fn();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<SandboxWizard transport={fake} onClose={onClose} />);
  });
  await settle(6);
  // The wizard is in a portal at the document root, so its parts are found from the body.
  return { fake, onClose, host: document.body };
}

const input = (h: HTMLElement, name: string) => h.querySelector<HTMLInputElement>(`input[name="${name}"]`)!;
const checked = (h: HTMLElement, name: string) => input(h, name).checked;
const calls = (fake: FakeSandboxTransport, method: string) => fake.calls.filter((c) => c.method === method);
const step = (h: HTMLElement) => h.querySelector('[data-step]')!.getAttribute('data-step');
const next = async (h: HTMLElement) => {
  await click(buttonByText(h, 'Next'));
  await settle();
};

describe('the steps', () => {
  it('opens on the look, with Ubuntu picked and the always-included list', async () => {
    const { host: h } = await mount();
    expect(step(h)).toBe('look');
    expect(hasText(h, 'How should the desktop look?')).toBe(true);
    expect(checked(h, 'sbx-look')).toBe(true); // first radio: Ubuntu
    const radios = [...h.querySelectorAll<HTMLInputElement>('input[name="sbx-look"]')];
    expect(radios.map((r) => r.checked)).toEqual([true, false, false]);
    for (const label of ['Ubuntu', 'Ubuntu with the Zorin look', 'Light desktop']) expect(hasText(h, label), label).toBe(true);
    expect(hasText(h, 'Always included: Chrome, a terminal, a file manager')).toBe(true);
    expect(buttonByText(h, 'Back')).toBeUndefined();
    expect(h.querySelector('.sbx-count')?.textContent).toBe('1 / 4');
  });

  it('look, then use (coding is checked), then the coding choices, then the summary', async () => {
    const { host: h } = await mount();
    await next(h);
    expect(step(h)).toBe('use');
    expect(checked(h, 'sbx-use-coding')).toBe(true);
    expect(checked(h, 'sbx-use-daily')).toBe(false);
    await next(h);
    expect(step(h)).toBe('coding');
    await next(h);
    expect(step(h)).toBe('summary');
    expect(buttonByText(h, 'Install')).toBeTruthy();
    expect(buttonByText(h, 'Next')).toBeUndefined();
  });

  it('daily use adds its own step after coding, and daily alone skips coding', async () => {
    const { host: h } = await mount();
    await next(h);
    await click(input(h, 'sbx-use-daily'));
    await next(h);
    expect(step(h)).toBe('coding');
    await next(h);
    expect(step(h)).toBe('daily');
    expect(hasText(h, "happen inside the sandbox's Chrome")).toBe(true);
    expect(hasText(h, 'Your sign-ins on this computer are not copied into it.')).toBe(true);
    await next(h);
    expect(step(h)).toBe('summary');
    // go back and take coding off: daily alone
    await click(buttonByText(h, 'Back'));
    await click(buttonByText(h, 'Back'));
    await click(buttonByText(h, 'Back'));
    expect(step(h)).toBe('use');
    await click(input(h, 'sbx-use-coding'));
    await next(h);
    expect(step(h)).toBe('daily');
    expect(h.querySelector('.sbx-count')?.textContent).toBe('3 / 4');
  });

  it('needs at least one use before Next', async () => {
    const { host: h } = await mount();
    await next(h);
    await click(input(h, 'sbx-use-coding'));
    expect(buttonByText(h, 'Next')!.disabled).toBe(true);
    await click(input(h, 'sbx-use-daily'));
    expect(buttonByText(h, 'Next')!.disabled).toBe(false);
  });

  it('Back keeps what was picked', async () => {
    const { host: h } = await mount();
    await click(h.querySelectorAll<HTMLInputElement>('input[name="sbx-look"]')[1]);
    await next(h);
    await click(buttonByText(h, 'Back'));
    expect([...h.querySelectorAll<HTMLInputElement>('input[name="sbx-look"]')].map((r) => r.checked)).toEqual([false, true, false]);
  });
});

describe('the coding choices', () => {
  it('pre-checks the languages the projects use and the package manager they use, and says so', async () => {
    const { host: h } = await mount();
    await next(h);
    await next(h);
    expect(step(h)).toBe('coding');
    expect(checked(h, 'sbx-lang-rust')).toBe(true);
    expect(checked(h, 'sbx-lang-python')).toBe(true);
    expect(checked(h, 'sbx-lang-go')).toBe(false);
    expect(checked(h, 'sbx-lang-java')).toBe(false);
    expect(hasText(h, 'Checked from your projects: Rust and Python.')).toBe(true);
    const pkgs = [...h.querySelectorAll<HTMLInputElement>('input[name="sbx-pkg"]')];
    expect(pkgs.map((r) => r.checked)).toEqual([false, true, false, false]); // npm, pnpm, yarn, bun
    expect(hasText(h, 'The one most of your projects use is picked.')).toBe(true);
    expect(hasText(h, 'Git, Node.js (current LTS), the GitHub tool and a C compiler are always included.')).toBe(true);
  });

  it('with nothing found it says so and checks nothing, with npm', async () => {
    const { host: h } = await mount({ detected: { languages: [], packageManager: null, projects: 5 } });
    await next(h);
    await next(h);
    expect([...h.querySelectorAll<HTMLInputElement>('input[name^="sbx-lang-"]')].some((r) => r.checked)).toBe(false);
    expect(hasText(h, 'Nothing was found in your projects. Pick what you need.')).toBe(true);
    expect([...h.querySelectorAll<HTMLInputElement>('input[name="sbx-pkg"]')][0].checked).toBe(true);
  });

  it('still works when looking at the projects fails', async () => {
    const { host: h } = await mount({ detectFails: true });
    await next(h);
    await next(h);
    expect(step(h)).toBe('coding');
    expect(hasText(h, 'Nothing was found in your projects. Pick what you need.')).toBe(true);
  });

  it('lists the extras from the recipe and starts with none picked', async () => {
    const { host: h } = await mount();
    await next(h);
    await next(h);
    const extras = [...h.querySelectorAll<HTMLInputElement>('input[name^="sbx-extra-"]')];
    expect(extras.map((e) => e.name)).toEqual(['sbx-extra-tmux', 'sbx-extra-sqlite', 'sbx-extra-ffmpeg']);
    expect(extras.some((e) => e.checked)).toBe(false);
  });
});

describe('the summary and Install', () => {
  async function toSummary(h: HTMLElement) {
    await next(h);
    await next(h);
    await next(h);
    expect(step(h)).toBe('summary');
  }

  it('lists the choices and the estimate, and installs nothing yet', async () => {
    const { fake, host: h } = await mount();
    await toSummary(h);
    const text = h.textContent ?? '';
    expect(text).toContain('Ubuntu');
    expect(text).toContain('Coding');
    expect(text).toContain('Rust and Python');
    expect(text).toContain('pnpm');
    const estimate = h.querySelector('[aria-label="Estimate"]')!.textContent!;
    expect(estimate).toMatch(/Download\s*About \d+(\.\d)? GB/);
    expect(estimate).toMatch(/Disk\s*About \d+\.\d GB, and about \d+\.\d GB more for the reset point/);
    expect(estimate).toMatch(/Time\s*About \d+ to \d+ minutes/);
    expect(hasText(h, 'It cannot see your files, programs or accounts.')).toBe(true);
    expect(calls(fake, 'install')).toEqual([]);
  });

  it('turning the reset point off drops its line from the estimate', async () => {
    const { host: h } = await mount();
    await toSummary(h);
    await click(h.querySelector<HTMLInputElement>('.sbx-check input'));
    expect(h.querySelector('[aria-label="Estimate"]')!.textContent).not.toContain('reset point');
  });

  it('Install sends the answers the person gave and shows the progress', async () => {
    const { fake, host: h } = await mount();
    await click(h.querySelectorAll<HTMLInputElement>('input[name="sbx-look"]')[1]); // Zorin
    await next(h);
    await click(input(h, 'sbx-use-daily'));
    await next(h);
    await click(input(h, 'sbx-lang-go'));
    await click(input(h, 'sbx-lang-rust')); // taken off again
    await click(input(h, 'sbx-extra-tmux'));
    await click([...h.querySelectorAll<HTMLInputElement>('input[name="sbx-pkg"]')][3]); // bun
    await next(h);
    await click([...h.querySelectorAll<HTMLInputElement>('input[name="sbx-media"]')][1]); // mpv
    await next(h);
    await click(buttonByText(h, 'Install'));
    await settle();
    expect(calls(fake, 'install')).toHaveLength(1);
    expect(calls(fake, 'install')[0].args[0]).toEqual({
      look: 'zorin',
      uses: ['coding', 'daily'],
      options: { 'coding.pkg': 'bun', 'coding.languages': 'python,go', 'coding.extras': 'tmux', 'daily.media': 'mpv', 'daily.images': 'eog' },
      resetPoint: true,
    });
    expect(step(h)).toBe('progress');
    expect(h.querySelector('[role="progressbar"]')!.getAttribute('aria-valuenow')).toBe('0');
  });

  it('shows a refused answer on the summary and stays there', async () => {
    const { host: h } = await mount({ installError: 'SANDBOX_BAD_OPTIONS: One of the choices is not one this version of the app offers.' });
    await toSummary(h);
    await click(buttonByText(h, 'Install'));
    await settle();
    expect(step(h)).toBe('summary');
    expect(h.querySelector('[role="alert"]')?.textContent).toBe('One of the choices is not one this version of the app offers.');
    expect(buttonByText(h, 'Install')!.disabled).toBe(false);
  });
});

describe('the progress', () => {
  async function installing() {
    const mounted = await mount();
    const h = mounted.host;
    await next(h);
    await next(h);
    await next(h);
    await click(buttonByText(h, 'Install'));
    await settle();
    return mounted;
  }

  it('a set up that ends before the app has even answered Install still shows its end', async () => {
    const mounted = await mount();
    const { fake, host: h } = mounted;
    await next(h);
    await next(h);
    await next(h);
    // the app finishes the whole set up (and says so) while the command is still on its way back
    const install = fake.install.bind(fake);
    fake.install = async (answers) => {
      await install(answers);
      fake.finishInstall();
    };
    await click(buttonByText(h, 'Install'));
    await settle();
    expect(step(h)).toBe('progress');
    expect(hasText(h, 'The sandbox is ready.')).toBe(true);
    expect(buttonByText(h, 'Done')).toBeDefined();
  });

  it('follows the app: the bar, the step name, the installer line and the step count', async () => {
    const { fake, host: h } = await installing();
    await act(async () => fake.pushInstall({ percent: 12, step: 'create', title: 'Downloading Ubuntu', index: 3, of: 8, detail: null }));
    expect(h.querySelector('[role="progressbar"]')!.getAttribute('aria-valuenow')).toBe('12');
    expect((h.querySelector('.sbx-bar > span') as HTMLElement).style.width).toBe('12%');
    expect(h.querySelector('.sbx-step-text')?.textContent).toBe('Downloading Ubuntu');
    expect(h.querySelector('.sbx-percent')?.textContent).toBe('12%');
    expect(hasText(h, 'Step 3 of 8')).toBe(true);
    await act(async () => fake.pushInstall({ percent: 55, step: 'apply', title: 'Installing the desktop and tools', index: 5, of: 8, detail: 'Installing XFCE desktop' }));
    expect(h.querySelector('.sbx-step-text')?.textContent).toBe('Installing XFCE desktop');
    expect(h.querySelector('[role="progressbar"]')!.getAttribute('aria-valuenow')).toBe('55');
    expect(hasText(h, 'You can close this window; set up keeps going.')).toBe(true);
  });

  it('Hide closes the window but not the set up; Stop set up asks the app to stop', async () => {
    const { fake, onClose, host: h } = await installing();
    await click(buttonByText(h, 'Stop set up'));
    await settle();
    expect(calls(fake, 'cancelInstall')).toHaveLength(1);
    expect(h.querySelector('.sbx-step-text')?.textContent).toBe('Stopping…');
    expect(buttonByText(h, 'Stopping…')!.disabled).toBe(true);
    await click(buttonByText(h, 'Hide'));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(calls(fake, 'dismissInstall')).toHaveLength(0);
  });

  it('ends with a done screen; Done forgets the set up and closes', async () => {
    const { fake, onClose, host: h } = await installing();
    await act(async () => fake.finishInstall());
    await settle();
    expect(hasText(h, 'The sandbox is ready.')).toBe(true);
    expect(hasText(h, 'stops again when it has been idle')).toBe(true);
    await click(buttonByText(h, 'Done'));
    expect(calls(fake, 'dismissInstall')).toHaveLength(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('a failed set up shows the plain reason, and Try again goes back to the summary', async () => {
    const { fake, host: h } = await installing();
    await act(async () => fake.failInstall('Set up failed at "Downloading Ubuntu": no internet.'));
    await settle();
    expect(h.querySelector('[role="alert"]')?.textContent).toBe('Set up failed at "Downloading Ubuntu": no internet.');
    expect(hasText(h, 'Nothing is left half-made.')).toBe(true);
    await click(buttonByText(h, 'Try again'));
    await settle();
    expect(step(h)).toBe('summary');
    expect(calls(fake, 'dismissInstall')).toHaveLength(1);
  });

  it('a stopped set up says nothing is left behind', async () => {
    const { fake, host: h } = await installing();
    await act(async () => fake.pushInstall({ state: 'cancelled', percent: 30 }));
    expect(hasText(h, 'Set up was stopped. Nothing is left behind.')).toBe(true);
    expect(buttonByText(h, 'Try again')).toBeTruthy();
  });

  it('opening the wizard while a set up runs goes straight to the bar', async () => {
    const running = {
      state: 'running' as const,
      percent: 41,
      step: 'apply',
      title: 'Installing the desktop and tools',
      index: 5,
      of: 8,
      detail: 'Installing Coding tools',
      cancelling: false,
      error: null,
      needsLinuxFeature: false,
    };
    const { host: h } = await mount({ status: { state: 'installing', install: running } });
    expect(step(h)).toBe('progress');
    expect(h.querySelector('[role="progressbar"]')!.getAttribute('aria-valuenow')).toBe('41');
    expect(h.querySelector('.sbx-step-text')?.textContent).toBe('Installing Coding tools');
  });
});

describe("Windows' Linux feature", () => {
  it('a computer without it starts on a step that explains it, and the button only opens a confirm dialog', async () => {
    const { fake, host: h } = await mount({ status: { linuxFeature: false } });
    expect(step(h)).toBe('linux');
    expect(hasText(h, 'It only happens when you press the button below.')).toBe(true);
    expect(calls(fake, 'linuxEnable')).toHaveLength(0);
    await click(buttonByText(h, "Turn on Windows' Linux feature…"));
    await settle();
    expect(calls(fake, 'linuxAsk')).toHaveLength(1);
    expect(h.querySelectorAll('[role="dialog"]')).toHaveLength(2);
    expect(calls(fake, 'linuxEnable')).toHaveLength(0);
  });

  it('Cancel on the dialog turns nothing on', async () => {
    const { fake, host: h } = await mount({ status: { linuxFeature: false } });
    await click(buttonByText(h, "Turn on Windows' Linux feature…"));
    await settle();
    const dialogs = h.querySelectorAll('[role="dialog"]');
    await click([...dialogs[dialogs.length - 1].querySelectorAll('button')].find((b) => b.textContent === 'Cancel'));
    await settle();
    expect(calls(fake, 'linuxEnable')).toHaveLength(0);
    expect(h.querySelectorAll('[role="dialog"]')).toHaveLength(1);
  });

  it('its button hands the dialog token to the app and then says to restart', async () => {
    const { fake, host: h } = await mount({ status: { linuxFeature: false } });
    await click(buttonByText(h, "Turn on Windows' Linux feature…"));
    await settle();
    await click(buttonByText(h, 'Turn on'));
    await settle();
    expect(calls(fake, 'linuxEnable').map((c) => c.args)).toEqual([['fake-token']]);
    expect(hasText(h, 'Restart the computer, then open Set up again.')).toBe(true);
    expect(buttonByText(h, "Turn on Windows' Linux feature…")).toBeUndefined();
  });

  it('an install the app refuses for the missing feature sends the person to that step', async () => {
    const { fake, host: h } = await mount();
    for (let i = 0; i < 3; i++) await next(h);
    fake.view.linuxFeature = false;
    await click(buttonByText(h, 'Install'));
    await settle();
    expect(step(h)).toBe('linux');
  });
});

describe('closing', () => {
  it('Cancel before Install closes and installs nothing', async () => {
    const { fake, onClose, host: h } = await mount();
    await next(h);
    await click(buttonByText(h, 'Cancel'));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(calls(fake, 'install')).toHaveLength(0);
  });

  it('Escape closes it before set up starts', async () => {
    const { onClose } = await mount();
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('Escape while a set up runs asks first; Cancel keeps the window, Close window hides it', async () => {
    const running = { ...emptyInstall(), state: 'running' as const, percent: 20, index: 1, of: 5, title: 'Downloading Ubuntu' };
    const { onClose, host: h } = await mount({ status: { state: 'installing', install: running } });
    const esc = async () =>
      act(async () => {
        document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      });
    await esc();
    expect(onClose).not.toHaveBeenCalled();
    expect(hasText(h, 'Set up is still running')).toBe(true);
    await click(buttonByText(h, 'Cancel'));
    expect(hasText(h, 'Set up is still running')).toBe(false);
    expect(onClose).not.toHaveBeenCalled();
    await esc();
    await click(buttonByText(h, 'Close window'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('keeps Tab inside the dialog and gives the focus back to the button that opened it', async () => {
    const opener = document.createElement('button');
    document.body.appendChild(opener);
    opener.focus();
    const { host: h } = await mount();
    const dialog = h.querySelector<HTMLElement>('[role="dialog"]')!;
    const items = [...dialog.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled])')];
    items[items.length - 1].focus();
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    });
    expect(document.activeElement).toBe(items[0]);
    await act(async () => root!.unmount());
    root = undefined;
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it('shows why when the recipes cannot be read', async () => {
    const fake = fakeSandboxTransport();
    fake.menu = () => Promise.reject(new Error('SANDBOX_GUEST_FILES_MISSING: The sandbox files that ship with the app are missing.'));
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root!.render(<SandboxWizard transport={fake} onClose={() => undefined} />);
    });
    await settle(6);
    expect(document.body.querySelector('[role="alert"]')?.textContent).toBe('The sandbox files that ship with the app are missing.');
  });
});
