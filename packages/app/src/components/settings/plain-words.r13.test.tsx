// @vitest-environment jsdom
// Round 13 #5: Settings > Phone and Settings > Bridge & MCP in plain words (UX-PHASE1 §0.3): no raw
// ids (`claude_cli_global`), no snake_case, no environment-variable names, no developer words such as
// "PWA", "dev tunnel", "tap target" or "input buffer" on the page itself.
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MobilePairingPanel } from '../MobilePairingPanel';
import { statusLine } from './tunnel-model';
import { backendName, ResultLines } from './sections/bridge/shared';
import { phoneRows } from './sections/PhoneSection';
import { mountRows, settle } from './test-utils';

vi.mock('qrcode', () => ({ default: { toCanvas: vi.fn(async () => undefined) } }));
vi.mock('../../lib/load-tunnel-dev-info', () => ({
  watchTunnelDevInfo: vi.fn((set: (info: unknown) => void) => {
    set({ tunnelUrl: 'https://quiet-lake-1234.trycloudflare.com', localUrl: 'http://127.0.0.1:1420' });
    return () => undefined;
  }),
}));
vi.mock('../../lib/tauri', () => ({
  tauri: {
    createMobilePairingSession: vi.fn(async () => ({
      pairing_code: 'ABCD1234',
      expires_at: 4102444800,
      server_public_key: 'pk',
      bridge_url: 'https://quiet-lake-1234.trycloudflare.com/bridge',
      qr_payload: {},
    })),
    listPairedMobileDevices: vi.fn(async () => []),
    revokePairedMobileDevice: vi.fn(async () => true),
  },
}));
vi.mock('./TunnelSection', async () => {
  const { createElement } = await import('react');
  return { TunnelSection: () => createElement('p', null, 'Tunnel stub') };
});

/** What a person can read on a rendered page: its text, labels, placeholders and tooltips. */
function visible(host: HTMLElement): string {
  const attrs = [...host.querySelectorAll('[aria-label],[placeholder],[title]')].flatMap((el) =>
    ['aria-label', 'placeholder', 'title'].map((a) => el.getAttribute(a) ?? ''),
  );
  return [host.textContent ?? '', ...attrs].join('\n');
}

const DEVELOPER_WORDS = /PWA|dev tunnel|Dev CLI|dev server|PUPPET_MASTER|tap target|input buffer|Vite|worktree/i;
const SNAKE_CASE = /\b[a-z]+_[a-z]+(_[a-z]+)*\b/;

let host: HTMLDivElement;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
});
afterEach(() => {
  host.remove();
});

describe('Settings > Phone', () => {
  it('the pairing block names no developer words, variables or ids, with and without a custom address', async () => {
    for (const publicPwaUrl of ['', 'https://pm.example.com']) {
      const root = createRoot(host);
      await act(async () =>
        root.render(<MobilePairingPanel publicPwaUrl={publicPwaUrl} devServerPort={1420} onPublicPwaUrlChange={vi.fn()} onDevServerPortChange={vi.fn()} />),
      );
      await settle(5);
      const text = visible(host);
      expect(text, `custom address "${publicPwaUrl}"`).not.toMatch(DEVELOPER_WORDS);
      expect(text.replace(/ABCD1234|pm-[a-z-]+/g, '')).not.toMatch(SNAKE_CASE);
      await act(async () => root.unmount());
    }
  });

  it('the rows around it say what they do without "tap target" or "input buffer"', async () => {
    const m = await mountRows(phoneRows.filter((row) => row.id !== 'pairing' && row.id !== 'paired-devices'));
    await settle(3);
    const text = visible(m.host);
    expect(text).not.toMatch(DEVELOPER_WORDS);
    expect(text).not.toMatch(SNAKE_CASE);
    await m.unmount();
  });

  it('the address line says "set outside the app", not the variable that set it', () => {
    const line = statusLine({ mode: 'env', state: 'running', error: null, url: 'https://x.example', config: { mode: 'quick', has_token: false } } as never);
    expect(line.text).toBe('Online (set outside the app)');
  });
});

describe('Settings > Bridge & MCP', () => {
  it('results of an install name the agent and where, never the internal backend id', async () => {
    const root = createRoot(host);
    const result = (backend: string) => ({ backend, installed: true, changed: true, message: 'ok' }) as never;
    await act(async () =>
      root.render(
        <ResultLines results={['claude_cli', 'claude_cli_global', 'claude_cli_global_home', 'codex_cli', 'codex_cli_global', 'opencode_cli', 'opencode_cli_global'].map(result)} />,
      ),
    );
    const text = visible(host);
    expect(text).not.toMatch(SNAKE_CASE);
    expect(text).toContain('Claude Code · this project');
    expect(text).toContain('Claude Code · all projects');
    expect(text).toContain('Codex · all projects');
    expect(text).toContain('OpenCode · this project');
    await act(async () => root.unmount());
  });

  it('an id nobody mapped yet is still read as words', () => {
    expect(backendName('kilo_cli_global')).toBe('Kilo · all projects');
    expect(backendName('something_new')).toBe('Something new');
  });
});
