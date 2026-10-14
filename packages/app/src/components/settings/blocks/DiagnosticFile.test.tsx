// @vitest-environment jsdom
// Settings > General > Save a diagnostic file: preview first, leave files out, save where the dialog says.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { buttonByText, byLabel, click, mountRows, settle, type Mounted } from '../test-utils';

const mocks = vi.hoisted(() => ({
  preview: {
    totalBytes: 3000,
    files: [
      { path: 'README.txt', label: 'A short note on what this file is', bytes: 800 },
      { path: 'settings.json', label: 'Your settings, with keys and tokens removed', bytes: 1200 },
      { path: 'recordings/op-1.cast', label: 'Recording of a run that failed, 2 hours ago', bytes: 1000 },
    ],
  },
  target: 'C:\out\diag.zip' as string | null,
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn(async () => mocks.target) }));
vi.mock('../../../lib/tauri', () => ({
  tauri: {
    diagnosticPreview: vi.fn(async () => mocks.preview),
    diagnosticShow: vi.fn(async (path: string) => `contents of ${path}`),
    diagnosticSave: vi.fn(async (path: string) => ({ path, files: 3, bytes: 2048 })),
  },
}));

import { tauri } from '../../../lib/tauri';
import { generalRows } from '../sections/GeneralSection';
import { DIAGNOSTIC_REFRESH_MS } from './DiagnosticFile';
import { act } from 'react';

const row = generalRows.filter((r) => r.id === 'diagnostic-file');
let m: Mounted;

beforeEach(async () => {
  vi.clearAllMocks();
  mocks.target = 'C:\out\diag.zip';
  m = await mountRows(row);
});
afterEach(async () => {
  await m.unmount();
});

describe('Save a diagnostic file', () => {
  it('shows nothing about the contents until Review is pressed, and writes nothing', async () => {
    expect(m.host.textContent).toContain('Save a diagnostic file');
    expect(m.host.textContent).toContain('Nothing is sent anywhere');
    expect(m.host.querySelector('.st-diag-list')).toBeNull();
    expect(tauri.diagnosticSave).not.toHaveBeenCalled();
  });

  it('lists every file in plain words with its size before saving', async () => {
    await click(buttonByText(m.host, 'Review…'));
    const items = [...m.host.querySelectorAll('.st-diag-item')].map((i) => i.textContent);
    expect(items).toHaveLength(3);
    expect(items[1]).toContain('Your settings, with keys and tokens removed');
    expect(items[1]).toContain('1 KB');
    expect(m.host.textContent).not.toContain('recordings/op-1.cast');
  });

  it('can show one file before it is saved', async () => {
    await click(buttonByText(m.host, 'Review…'));
    await click(m.host.querySelectorAll('.st-diag-item button')[1]);
    expect(m.host.querySelector('.st-diag-text')?.textContent).toBe('contents of settings.json');
  });

  it('saves to the chosen file and leaves out what was unticked', async () => {
    await click(buttonByText(m.host, 'Review…'));
    await click(byLabel(m.host, 'Recording of a run that failed, 2 hours ago'));
    await click(buttonByText(m.host, 'Save…'));
    await settle();
    expect(tauri.diagnosticSave).toHaveBeenCalledWith('C:\out\diag.zip', ['recordings/op-1.cast']);
    expect(m.host.textContent).toContain('Saved to C:\out\diag.zip');
    expect(m.host.querySelector('.st-diag-list')).toBeNull();
  });

  it('does nothing when the save dialog is cancelled', async () => {
    mocks.target = null;
    await click(buttonByText(m.host, 'Review…'));
    await click(buttonByText(m.host, 'Save…'));
    await settle();
    expect(tauri.diagnosticSave).not.toHaveBeenCalled();
    expect(m.host.querySelector('.st-diag-list')).not.toBeNull();
  });
});

describe('the open list keeps itself current', () => {
  const newFailure = { path: 'recordings/op-2.cast', label: 'Recording of a run that failed, just now', bytes: 500 };
  const after = { totalBytes: 3500, files: [...mocks.preview.files, newFailure] };

  beforeEach(() => vi.mocked(tauri.diagnosticPreview).mockReset().mockResolvedValue(mocks.preview));
  afterEach(() => vi.useRealTimers());

  it('lists a run that failed while it was open, without pressing Refresh', async () => {
    vi.useFakeTimers();
    await click(buttonByText(m.host, 'Review…'));
    expect(m.host.querySelectorAll('.st-diag-item')).toHaveLength(3);
    vi.mocked(tauri.diagnosticPreview).mockResolvedValueOnce(after);
    await act(async () => void (await vi.advanceTimersByTimeAsync(DIAGNOSTIC_REFRESH_MS + 50)));
    expect(m.host.querySelectorAll('.st-diag-item')).toHaveLength(4);
    expect(m.host.textContent).toContain('Recording of a run that failed, just now');
  });

  it('re-reads when the window is focused again, and keeps what was unticked', async () => {
    await click(buttonByText(m.host, 'Review…'));
    await click(byLabel(m.host, 'Your settings, with keys and tokens removed'));
    vi.mocked(tauri.diagnosticPreview).mockResolvedValueOnce(after);
    await act(async () => void window.dispatchEvent(new Event('focus')));
    await settle();
    expect(m.host.querySelectorAll('.st-diag-item')).toHaveLength(4);
    expect(byLabel(m.host, 'Your settings, with keys and tokens removed').checked).toBe(false);
    expect(byLabel(m.host, 'Recording of a run that failed, just now').checked).toBe(true);
  });

  it('does not read at all while the list is closed', async () => {
    vi.useFakeTimers();
    await act(async () => void (await vi.advanceTimersByTimeAsync(DIAGNOSTIC_REFRESH_MS * 3)));
    expect(tauri.diagnosticPreview).not.toHaveBeenCalled();
  });
});
