// The click behind every desktop answer (live v0.2.0 hunt: five trust answers in 400 ms). The
// backend refuses a yes or a folder-trust answer without a fresh, trusted gesture
// (`src-tauri/src/pane_prompt/answers.rs`); these tests keep the UI side honest: a gesture is made
// only from an event, and every place that answers a pane prompt is known.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';
import { gestureOf } from './user-gesture';

describe('gestureOf', () => {
  it('carries the event: trusted only when the browser says so, a fresh id each time', () => {
    const real = gestureOf({ isTrusted: true, type: 'click' });
    expect(real).toMatchObject({ trusted: true, kind: 'click' });
    expect(Math.abs(real.atMs - Date.now())).toBeLessThan(1_000);
    expect(gestureOf({ isTrusted: true, type: 'click' }).id).not.toBe(real.id);
    // `element.click()` from code is not trusted; a missing flag is not trusted either.
    expect(gestureOf({ isTrusted: false, type: 'click' }).trusted).toBe(false);
    expect(gestureOf({ type: 'click' }).trusted).toBe(false);
  });

  it('reads the native event behind a React event', () => {
    expect(gestureOf({ type: 'click', nativeEvent: { isTrusted: true, type: 'keydown' } })).toMatchObject({ trusted: true, kind: 'keydown' });
  });
});

const SRC = join(__dirname, '..');

function sources(dir = SRC): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === 'dev' ? [] : sources(path);
    return /\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name) ? [path] : [];
  });
}

function callers(needle: string): string[] {
  return sources()
    .flatMap((file) => {
      const code = readFileSync(file, 'utf8')
        .split('\n')
        .filter((line) => !line.trim().startsWith('//') && !line.trim().startsWith('*'))
        .join('\n');
      return Array.from({ length: code.split(needle).length - 1 }, () => relative(SRC, file).replace(/\\/g, '/'));
    })
    .sort();
}

describe('every answer to a pane prompt', () => {
  it('goes through two known places with a click, and one known refusal without', () => {
    // With the person's click (the gesture is a required argument, so a new caller must have one).
    expect(callers('.answerPanePrompt(')).toEqual(['components/simple/SimpleProject.tsx', 'hooks/useCockpit.ts']);
    // The only answer without a click: Simple mode's own No to a high-risk command.
    expect(callers('.declinePanePrompt(')).toEqual(['components/simple/SimpleProject.tsx']);
    // Nothing else invokes the command directly.
    expect(callers("'answer_pane_prompt'")).toEqual(['lib/cockpit-api.ts', 'lib/cockpit-api.ts']);
  });

  it('the cockpit and Simple mode buttons make the gesture from the click event', () => {
    for (const file of ['components/cockpit/ApprovalPrompt.tsx', 'components/cockpit/Inbox.tsx', 'components/cockpit/PaneView.tsx', 'components/simple/Cards.tsx']) {
      const code = readFileSync(join(SRC, file), 'utf8');
      expect(code, file).toMatch(/onClick=\{\(event\) => [^}]*gestureOf\(event\)/);
    }
  });
});
