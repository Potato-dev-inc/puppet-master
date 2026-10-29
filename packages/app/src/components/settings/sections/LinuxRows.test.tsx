// @vitest-environment jsdom
// Settings > Agents > Run agents in Linux: each WSL state, Copy puts the exact command on the clipboard, Check shows the
// spinner and then the checked and found lines, "Runs on" saves per agent, and an agent with a reason cannot go to Linux.
// Fixtures use fake distro and agent ids only; the commands are mocked, so nothing starts WSL.
import { act } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { wslCheck, wslPlacesGet, wslPlaceSet, wslStatus, type AgentPlaceRow, type DistroView, type WslStatus } from '../../../lib/wsl';
import { buttonByText, byLabel, click, hasText, mountRows, pickOption, settle, type Mounted } from '../test-utils';
import {
  checkedText,
  foundText,
  isLockedToWindows,
  linuxErrorText,
  linuxOptions,
  LinuxRows,
  WINDOWS_VALUE,
} from './LinuxRows';

vi.mock('../../../lib/wsl', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../../lib/wsl')>();
  return { ...real, wslStatus: vi.fn(), wslCheck: vi.fn(), wslPlacesGet: vi.fn(), wslPlaceSet: vi.fn() };
});

const READY: WslStatus = {
  state: 'ready',
  distros: [
    { name: 'Ubuntu-Test', default: true, version: 2, state: 'Running' },
    { name: 'Debian-Test', default: false, version: 1, state: 'Stopped' },
  ],
};

const PLACES: AgentPlaceRow[] = [
  { agentId: 'fake_a', label: 'Agent A', distro: null, found: null, whyNot: null },
  { agentId: 'fake_b', label: 'Agent B', distro: 'Ubuntu-Test', found: null, whyNot: null },
  { agentId: 'fake_c', label: 'Agent C', distro: null, found: null, whyNot: "Agent C can't run in Linux from Puppet Master yet." },
];

const VIEW: DistroView = {
  name: 'Ubuntu-Test',
  version: 2,
  default: true,
  checkedAtMs: Date.UTC(2026, 9, 29, 10, 42),
  git: '2.43',
  interop: true,
  networking: null,
  agents: [
    { agentId: 'fake_a', label: 'Agent A', distro: 'Ubuntu-Test', found: true, whyNot: null },
    { agentId: 'fake_b', label: 'Agent B', distro: 'Ubuntu-Test', found: false, whyNot: null },
  ],
};

let m: Mounted;

async function mount(status: WslStatus | Error, places: AgentPlaceRow[] = PLACES) {
  if (status instanceof Error) vi.mocked(wslStatus).mockRejectedValue(status);
  else vi.mocked(wslStatus).mockResolvedValue(status);
  vi.mocked(wslPlacesGet).mockResolvedValue(places);
  vi.mocked(wslPlaceSet).mockImplementation(async (agentId: string, distro: string | null) => ({
    ...(places.find((r) => r.agentId === agentId) ?? PLACES[0]),
    distro,
  }));
  m = await mountRows([
    { id: 'linux', label: 'Run agents in Linux (WSL)', group: 'Linux', render: (env) => <LinuxRows onSaved={env.markSaved} /> },
  ]);
  await settle(10);
  return m;
}

const runsOn = (label: string) => byLabel<HTMLSelectElement>(m.host, `Runs on for ${label}`)!;
const distroLine = (name: string) => m.host.querySelector<HTMLElement>(`[data-distro="${name}"]`)!;
const agentLine = (id: string) => m.host.querySelector<HTMLElement>(`[data-agent-id="${id}"]`)!;

beforeEach(() => {
  vi.clearAllMocks();
  Object.defineProperty(navigator, 'clipboard', { value: { writeText: vi.fn(async () => undefined) }, configurable: true });
});

afterEach(async () => {
  await m?.unmount();
});

describe('Run agents in Linux: the WSL states', () => {
  it('is hidden on a build without WSL', async () => {
    await mount({ state: 'not_windows' });
    expect(hasText(m.host, 'Run agents in Linux (WSL)')).toBe(false);
    expect(hasText(m.host, 'Runs on')).toBe(false);
  });

  it('not installed: says the install command, and Copy puts that exact command on the clipboard', async () => {
    await mount({ state: 'not_installed', command: 'wsl --install' });
    expect(hasText(m.host, 'WSL is not installed. To use Linux, open PowerShell as administrator and run: wsl --install')).toBe(true);
    expect(hasText(m.host, 'Puppet Master does not install it for you.')).toBe(true);
    await click(buttonByText(m.host, 'Copy'));
    expect(navigator.clipboard.writeText).toHaveBeenCalledExactlyOnceWith('wsl --install');
  });

  it('not installed: Check again asks the status again, without the cache', async () => {
    await mount({ state: 'not_installed', command: 'wsl --install' });
    vi.mocked(wslStatus).mockResolvedValue(READY);
    await click(buttonByText(m.host, 'Check again'));
    await settle();
    expect(wslStatus).toHaveBeenLastCalledWith(true);
    expect(distroLine('Ubuntu-Test')).toBeTruthy();
  });

  it('no distro: says to install Ubuntu, and Copy puts that exact command on the clipboard', async () => {
    await mount({ state: 'no_distro', command: 'wsl --install -d Ubuntu' });
    expect(hasText(m.host, 'WSL has no Linux yet. Run: wsl --install -d Ubuntu')).toBe(true);
    await click(buttonByText(m.host, 'Copy'));
    expect(navigator.clipboard.writeText).toHaveBeenCalledExactlyOnceWith('wsl --install -d Ubuntu');
  });

  it('broken: says WSL did not answer and shows the first line it printed', async () => {
    await mount({ state: 'broken', line: 'Unexpected output' });
    expect(hasText(m.host, 'WSL did not answer as expected.')).toBe(true);
    expect(hasText(m.host, 'Unexpected output')).toBe(true);
    expect(buttonByText(m.host, 'Check again')).toBeTruthy();
  });

  it('a browser preview with no desktop shell says it is only in the desktop app', async () => {
    await mount(new Error('Desktop shell required: wsl_status'));
    expect(hasText(m.host, 'Available in the desktop app.')).toBe(true);
    expect(buttonByText(m.host, 'Copy')).toBeUndefined();
  });
});

describe('Run agents in Linux: distros and checks', () => {
  it('lists each distro with its WSL version; a WSL 1 distro says so and has no Check button', async () => {
    await mount(READY);
    expect(hasText(m.host, 'Ubuntu-Test (default) · WSL 2')).toBe(true);
    expect(hasText(m.host, 'Debian-Test · WSL 1')).toBe(true);
    expect(hasText(m.host, 'Uses WSL 1. Agents need WSL 2: wsl --set-version Debian-Test 2')).toBe(true);
    expect(buttonByText(m.host, 'Check Ubuntu-Test')!.disabled).toBe(false);
    expect(buttonByText(m.host, 'Check Debian-Test')!.disabled).toBe(true);
  });

  it('Check shows the spinner, then the checked line and the found line', async () => {
    let finish!: (view: DistroView) => void;
    vi.mocked(wslCheck).mockReturnValue(new Promise<DistroView>((resolve) => (finish = resolve)));
    await mount(READY);
    expect(hasText(m.host, 'Not checked yet')).toBe(true);

    await click(buttonByText(m.host, 'Check Ubuntu-Test'));
    await settle();
    expect(wslCheck).toHaveBeenCalledExactlyOnceWith('Ubuntu-Test');
    expect(byLabel(m.host, 'Checking Ubuntu-Test')).toBeTruthy();
    expect(hasText(m.host, 'Checking Ubuntu-Test. Starting it can take a few seconds.')).toBe(true);
    expect(buttonByText(m.host, 'Check Ubuntu-Test')!.disabled).toBe(true);

    await act(async () => finish(VIEW));
    await settle();
    expect(byLabel(m.host, 'Checking Ubuntu-Test')).toBeNull();
    expect(hasText(m.host, 'git 2.43 · Windows programs: on')).toBe(true);
    expect(hasText(m.host, 'Checked ')).toBe(true);
    expect(hasText(m.host, 'Not found in Ubuntu-Test. Install it inside Linux, then check again.')).toBe(true);
    expect(buttonByText(m.host, 'Check Ubuntu-Test')!.disabled).toBe(false);
  });

  it('a failed check shows the plain sentence without its code', async () => {
    vi.mocked(wslCheck).mockRejectedValue(new Error('WSL_PROBE_TIMEOUT: Linux took too long to answer. Try again.'));
    await mount(READY);
    await click(buttonByText(m.host, 'Check Ubuntu-Test'));
    await settle();
    const alert = m.host.querySelector('[role="alert"]');
    expect(alert?.textContent).toBe('Linux took too long to answer. Try again.');
    expect(hasText(m.host, 'WSL_PROBE_TIMEOUT')).toBe(false);
  });

  it('says nothing about Windows agents when no agent is set to Linux', async () => {
    await mount(READY, [PLACES[0]]);
    expect(hasText(m.host, 'Not checked yet')).toBe(false);
    expect(agentLine('fake_a').textContent).not.toContain('Found');
  });
});

describe('Run agents in Linux: Runs on per agent', () => {
  it('offers Windows and each WSL 2 distro, not a WSL 1 one', async () => {
    await mount(READY);
    const options = [...runsOn('Agent A').options].map((o) => o.textContent);
    expect(options).toEqual(['Windows', 'Linux · Ubuntu-Test']);
  });

  it('choosing Linux saves that distro for that agent only, and ticks the saved mark', async () => {
    await mount(READY);
    await pickOption(runsOn('Agent A'), 'Ubuntu-Test');
    await settle();
    expect(wslPlaceSet).toHaveBeenCalledExactlyOnceWith('fake_a', 'Ubuntu-Test');
    expect(m.env.markSaved).toHaveBeenCalled();
    expect(agentLine('fake_a').querySelector('select')!.value).toBe('Ubuntu-Test');
  });

  it('choosing Windows saves null (always allowed)', async () => {
    await mount(READY);
    await pickOption(runsOn('Agent B'), WINDOWS_VALUE);
    await settle();
    expect(wslPlaceSet).toHaveBeenCalledExactlyOnceWith('fake_b', null);
  });

  it('an agent with a reason cannot go to Linux: the select and the Linux option are disabled, the reason shows', async () => {
    await mount(READY);
    expect(isLockedToWindows(PLACES[2])).toBe(true);
    expect(runsOn('Agent C').disabled).toBe(true);
    expect(hasText(m.host, "Agent C can't run in Linux from Puppet Master yet.")).toBe(true);
  });

  it('an agent that is on Linux with a reason can still be set back to Windows', async () => {
    const onLinux: AgentPlaceRow = { agentId: 'fake_d', label: 'Agent D', distro: 'Ubuntu-Test', found: null, whyNot: "Agent D can't run in Linux from Puppet Master yet." };
    await mount(READY, [onLinux]);
    expect(isLockedToWindows(onLinux)).toBe(false);
    expect(runsOn('Agent D').disabled).toBe(false);
    expect([...runsOn('Agent D').options].find((o) => o.value === 'Ubuntu-Test')!.disabled).toBe(true);
    await pickOption(runsOn('Agent D'), WINDOWS_VALUE);
    await settle();
    expect(wslPlaceSet).toHaveBeenCalledExactlyOnceWith('fake_d', null);
  });

  it('a refused save shows the server sentence on the block', async () => {
    await mount(READY);
    vi.mocked(wslPlaceSet).mockRejectedValue(new Error('WSL_DISTRO_NOT_V2: Ubuntu-Test uses WSL 1. Agents need WSL 2.'));
    await pickOption(runsOn('Agent A'), 'Ubuntu-Test');
    await settle();
    expect(m.host.querySelector('[role="alert"]')?.textContent).toBe('Ubuntu-Test uses WSL 1. Agents need WSL 2.');
  });

  it('with no agent able to run in Linux, says so', async () => {
    await mount(READY, []);
    expect(hasText(m.host, 'No agents can run in Linux yet.')).toBe(true);
  });

  it('the footer says Linux is not a sandbox and keeps Windows logins out', async () => {
    await mount(READY);
    expect(hasText(m.host, 'In Linux an agent has its own sign-in. Puppet Master never copies your Windows logins or keys into Linux.')).toBe(true);
    expect(hasText(m.host, 'Linux is not a sandbox')).toBe(true);
  });
});

describe('Run agents in Linux: pure helpers', () => {
  it('linuxErrorText drops the code and falls back when there is no text', () => {
    expect(linuxErrorText(new Error('WSL_NO_DISTRO: WSL has no Linux yet.'))).toBe('WSL has no Linux yet.');
    expect(linuxErrorText('plain text')).toBe('plain text');
    expect(linuxErrorText(new Error('  '))).toBe("Couldn't do that. Try again.");
  });

  it('linuxOptions keeps WSL 2 distros and keeps the one an agent already uses', () => {
    const distros = [
      { name: 'Ubuntu-Test', default: true, version: 2, state: 'Running' },
      { name: 'Debian-Test', default: false, version: 1, state: 'Stopped' },
    ];
    expect(linuxOptions(distros, PLACES[0])).toEqual(['Ubuntu-Test']);
    expect(linuxOptions(distros, { ...PLACES[0], distro: 'Gone-Test' })).toEqual(['Ubuntu-Test', 'Gone-Test']);
  });

  it('foundText: nothing for Windows, "Not checked yet" before a check, then the check decides', () => {
    expect(foundText(PLACES[0], {})).toBeNull();
    expect(foundText(PLACES[1], {})).toBe('Not checked yet');
    expect(foundText(PLACES[1], { 'Ubuntu-Test': VIEW })).toBe('Not found in Ubuntu-Test. Install it inside Linux, then check again.');
    expect(foundText({ ...PLACES[1], agentId: 'fake_a' }, { 'Ubuntu-Test': VIEW })).toBe('Found in Ubuntu-Test');
  });

  it('checkedText gives the time, git (or "not found") and Windows programs', () => {
    expect(checkedText(VIEW)).toMatch(/^Checked /);
    expect(checkedText(VIEW)).toContain('git 2.43 · Windows programs: on');
    expect(checkedText({ ...VIEW, git: null, interop: false })).toContain('git not found · Windows programs: off');
  });
});
