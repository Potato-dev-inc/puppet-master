import { describe, expect, it } from 'vitest';
import { isSeparateCopyCwd, projectLabel, projectWithCopy } from './shell-labels';

const COPY = 'C:\\Users\\me\\shop\\.puppet-master\\worktrees\\7d4cb2b3-2-209c';

describe('project names in the shell', () => {
  it('names a plain folder by its last part', () => {
    expect(projectLabel('C:\\Users\\me\\shop')).toBe('shop');
    expect(projectLabel('/home/me/shop/')).toBe('shop');
    expect(projectWithCopy('/home/me/shop')).toBe('shop');
    expect(isSeparateCopyCwd('/home/me/shop')).toBe(false);
  });

  it('names a separate copy by its project, never by the id of the copy folder', () => {
    expect(projectLabel(COPY)).toBe('shop');
    expect(projectLabel('/home/me/shop/.puppet-master/worktrees/h-1a2b')).toBe('shop');
    expect(projectLabel(COPY)).not.toContain('7d4cb2b3');
    expect(projectWithCopy(COPY)).toBe('separate copy of shop');
    expect(isSeparateCopyCwd(COPY)).toBe(true);
  });

  it('copes with no folder', () => {
    expect(projectLabel(undefined)).toBe('');
    expect(projectWithCopy(null)).toBe('');
  });
});
