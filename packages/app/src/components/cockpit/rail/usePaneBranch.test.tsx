// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { usePaneBranch } from './usePaneBranch';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let latest: string | null = null;
const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
function Probe({ api, status }: { api: { paneGitBranch: (id: string) => Promise<string | null> }; status: string }) {
  latest = usePaneBranch('p1', status, true, api);
  return null;
}
beforeEach(() => {
  latest = null;
  host = document.createElement('div');
  root = createRoot(host);
});
afterEach(() => act(() => root.unmount()));

describe('usePaneBranch', () => {
  it('returns the branch and looks again when the status changes', async () => {
    const api = { paneGitBranch: vi.fn().mockResolvedValueOnce('main').mockResolvedValueOnce('feature') };
    await act(async () => root.render(<Probe api={api} status="running" />));
    await flush();
    expect(latest).toBe('main');
    await act(async () => root.render(<Probe api={api} status="idle" />));
    await flush();
    expect(latest).toBe('feature');
  });
  it('is null for a folder with no git and when the lookup fails', async () => {
    await act(async () => root.render(<Probe api={{ paneGitBranch: async () => null }} status="idle" />));
    await flush();
    expect(latest).toBeNull();
    await act(async () =>
      root.render(
        <Probe
          api={{
            paneGitBranch: async () => {
              throw new Error('no shell');
            },
          }}
          status="running"
        />,
      ),
    );
    await flush();
    expect(latest).toBeNull();
  });
});
