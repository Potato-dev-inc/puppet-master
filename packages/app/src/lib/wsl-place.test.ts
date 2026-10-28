import { describe, expect, it } from 'vitest';
import type { PanePlace } from './wsl';
import { placeLabel, placeTooltip, toWindowsPath } from './wsl-place';

const place: PanePlace = {
  kind: 'linux',
  distro: 'Ubuntu',
  cwd: '/mnt/d/work/shop',
  mount_root: '/mnt/',
  unc_root: '\\\\wsl.localhost\\Ubuntu',
  home_windows: '\\\\wsl.localhost\\Ubuntu\\home\\you',
};

describe('placeLabel and placeTooltip', () => {
  it('labels a Linux pane with its distro and nothing else', () => {
    expect(placeLabel(place)).toBe('Linux · Ubuntu');
    expect(placeLabel(null)).toBeNull();
    expect(placeLabel(undefined)).toBeNull();
    expect(placeLabel({ ...place, kind: 'windows' })).toBeNull();
  });

  it('says where it runs, then one line per warning', () => {
    expect(placeTooltip(place)).toBe('Runs in Ubuntu (WSL) in /mnt/d/work/shop');
    expect(placeTooltip({ ...place, warnings: ['Line endings differ.', 'Second note.'] })).toBe(
      'Runs in Ubuntu (WSL) in /mnt/d/work/shop\nLine endings differ.\nSecond note.',
    );
    expect(placeTooltip(null)).toBe('');
  });
});

describe('toWindowsPath (mirror of wsl::path::to_windows)', () => {
  const drives: [string, string][] = [
    ['/mnt/c/Users/you/shop', 'C:\\Users\\you\\shop'],
    ['/mnt/d/a b/c', 'D:\\a b\\c'],
    ['/mnt/d', 'D:\\'],
    ['/mnt/d/', 'D:\\'],
    ['/mnt/D/x', 'D:\\x'],
    ['/mnt/d/work/Café 文件', 'D:\\work\\Café 文件'],
  ];

  it.each(drives)('maps the Windows drive row %s', (linux, windows) => {
    expect(toWindowsPath(place, linux)).toEqual({ ok: true, path: windows });
  });

  it('maps a folder inside the distro to the UNC root', () => {
    expect(toWindowsPath(place, '/home/you/p')).toEqual({ ok: true, path: '\\\\wsl.localhost\\Ubuntu\\home\\you\\p' });
    expect(toWindowsPath(place, '/home/you/Ünï code')).toEqual({
      ok: true,
      path: '\\\\wsl.localhost\\Ubuntu\\home\\you\\Ünï code',
    });
  });

  it('does not take a longer name for a drive', () => {
    expect(toWindowsPath(place, '/mnt/dx/y')).toEqual({ ok: true, path: '\\\\wsl.localhost\\Ubuntu\\mnt\\dx\\y' });
    expect(toWindowsPath(place, '/mnt')).toEqual({ ok: true, path: '\\\\wsl.localhost\\Ubuntu\\mnt' });
  });

  it('uses the pane mount root, so wsl.conf root=/ maps too', () => {
    const rootMounted: PanePlace = { ...place, mount_root: '/' };
    expect(toWindowsPath(rootMounted, '/c/x')).toEqual({ ok: true, path: 'C:\\x' });
    expect(toWindowsPath(rootMounted, '/home/you')).toEqual({ ok: true, path: '\\\\wsl.localhost\\Ubuntu\\home\\you' });
  });

  it('refuses relative and empty paths instead of guessing', () => {
    expect(toWindowsPath(place, 'work/shop')).toEqual({ ok: false, error: 'relative' });
    expect(toWindowsPath(place, '')).toEqual({ ok: false, error: 'empty' });
  });
});
