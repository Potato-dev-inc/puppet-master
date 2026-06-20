import { describe, expect, it } from 'vitest';
import {
  snapshotWithLocalInputOverlay,
  updateLocalInputOverlay,
  type LocalInputOverlay,
} from './canvas-terminal';

const EMPTY: LocalInputOverlay = { anchorLine: null, text: '' };

describe('CanvasTerminal local input overlay', () => {
  it('renders typed text at the current prompt line', () => {
    const overlay = updateLocalInputOverlay('ren@Mac % ', EMPTY, 'git');

    expect(snapshotWithLocalInputOverlay('ren@Mac % ', overlay)).toBe('ren@Mac % git');
  });

  it('keeps deliberate spaces visible immediately', () => {
    const overlay = updateLocalInputOverlay('ren@Mac % ', EMPTY, 'git status');

    expect(snapshotWithLocalInputOverlay('ren@Mac % ', overlay)).toBe('ren@Mac % git status');
  });

  it('erases one local CJK character on the first backspace after remote echo', () => {
    const typed = updateLocalInputOverlay('> ', EMPTY, '你好');
    const erased = updateLocalInputOverlay('> 你好', typed, '\x7f');

    expect(snapshotWithLocalInputOverlay('> 你好', erased)).toBe('> 你');
  });

  it('does not erase prompt text when there is no local input', () => {
    const overlay = updateLocalInputOverlay('> ', EMPTY, '\x7f');

    expect(snapshotWithLocalInputOverlay('> ', overlay)).toBe('> ');
  });

  it('clears local input on enter', () => {
    const typed = updateLocalInputOverlay('> ', EMPTY, 'git status');
    const submitted = updateLocalInputOverlay('> git status', typed, '\r');

    expect(snapshotWithLocalInputOverlay('> git status', submitted)).toBe('> git status');
  });
});
