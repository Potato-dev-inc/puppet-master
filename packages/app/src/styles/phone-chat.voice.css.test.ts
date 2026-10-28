// @vitest-environment node
// The mic on the phone's message box: a 44px touch target, and the recording dot does not pulse for people who
// ask the system for less motion.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// Git on Windows may check the file out with CRLF line endings; the rules are read the same either way.
const css = readFileSync(new URL('./phone-chat.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');

describe('voice typing styles on the phone', () => {
  it('the mic is a 44px touch target', () => {
    const start = css.indexOf('.ph .ph-chat .ck-chat-mic {');
    expect(start).toBeGreaterThan(-1);
    const body = css.slice(start, css.indexOf('}', start));
    expect(body).toMatch(/width:\s*44px/);
    expect(body).toMatch(/height:\s*44px/);
  });

  it('the recording dot stops pulsing under reduced motion', () => {
    const start = css.indexOf('@media (prefers-reduced-motion: reduce) {\n  .ph .ph-chat .ck-chat-mic-dot');
    expect(start).toBeGreaterThan(-1);
    const body = css.slice(start, css.indexOf('}', css.indexOf('}', start) + 1) + 1);
    expect(body).toMatch(/animation:\s*none/);
  });
});
