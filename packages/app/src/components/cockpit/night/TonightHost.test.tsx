// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ShipApi } from '../../../lib/ship-api';
import { TonightHost, tonightProjects } from './TonightHost';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const seen = vi.hoisted(() => ({ props: null as null | { projects: unknown[]; undoOn: boolean } }));
vi.mock('./TonightSheet', () => ({
  TonightSheet: (props: { projects: unknown[]; undoOn: boolean }) => {
    seen.props = props;
    return null;
  },
}));

const recent = (path: string, name: string, state: string) => ({ path, name, state: state as never, last_used_ms: 1 });

describe('tonightProjects', () => {
  it('lists pane folders first (a copy maps back to its project), then recents, each once; only repos are git', () => {
    const list = tonightProjects(
      [recent('C:\\work\\shop', 'shop', 'repo'), recent('C:\\work\\notes', 'notes', 'folder'), recent('C:\\work\\gone', 'gone', 'missing')],
      ['C:\\work\\notes', 'C:\\work\\shop\\.puppet-master\\worktrees\\ab-1', 'C:\\work\\shop', undefined],
    );
    expect(list).toEqual([
      { path: 'C:\\work\\notes', name: 'notes', git: false },
      { path: 'C:\\work\\shop', name: 'shop', git: true },
    ]);
  });

  it('uses the looked-up Git state for a pane folder that is not in the recents', () => {
    const list = tonightProjects([], ['C:\\work\\lab'], { 'C:\\work\\lab': 'repo' });
    expect(list).toEqual([{ path: 'C:\\work\\lab', name: 'lab', git: true }]);
  });
});

describe('TonightHost', () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    seen.props = null;
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });

  it('gives the sheet the folders and the Undo switch (on when it cannot be read)', async () => {
    const ship = { recent: vi.fn().mockResolvedValue([recent('C:\\work\\shop', 'shop', 'repo')]), gitState: vi.fn().mockResolvedValue('folder') } as unknown as ShipApi;
    const c = { registry: { paneList: [{ info: { cwd: 'C:\\work\\lab' } }] }, notify: vi.fn() } as never;
    await act(async () => root.render(<TonightHost c={c} onClose={() => {}} ship={ship} />));
    await act(async () => {});
    expect(seen.props?.undoOn).toBe(true);
    expect(seen.props?.projects).toEqual([
      { path: 'C:\\work\\lab', name: 'lab', git: false },
      { path: 'C:\\work\\shop', name: 'shop', git: true },
    ]);
  });
});
