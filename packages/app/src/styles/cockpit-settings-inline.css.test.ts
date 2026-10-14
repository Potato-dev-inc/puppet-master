// @vitest-environment node
// The cockpit resets every <button> (`.cockpit button { background: none; padding: 0 }`). That reset beats the
// Tailwind classes of the older Settings blocks (Toolbelt, agent definitions, pairing), so inside the inline
// Settings view their buttons and switches lost their look. The reset must skip anything under `.settings`.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./cockpit.css', import.meta.url), 'utf8');

describe('cockpit button reset vs inline Settings', () => {
  it('does not apply to buttons inside .settings', () => {
    expect(css).toMatch(/\.cockpit button:not\(:where\(\.settings \*\)\)\s*\{[^}]*background:\s*none/);
    expect(css).not.toMatch(/^\.cockpit button\s*\{/m);
  });

  it('the inline panel fills the main area instead of the window', () => {
    const settings = readFileSync(new URL('./settings.css', import.meta.url), 'utf8');
    expect(settings).toMatch(/\.settings\.settings--inline\s*\{[^}]*position:\s*absolute/);
  });
});
