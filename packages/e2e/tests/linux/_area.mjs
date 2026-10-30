// Helpers for the Linux Inside tests. Not a test.
import { openSettings, scrollToCss } from '../night/_area.mjs';

/** Open Settings > Agents and wait until the Linux block has said what WSL looks like on this PC. */
export async function openLinuxBlock(t) {
  await openSettings(t, 'Agents');
  await t.expect.text('Run agents in Linux (WSL)');
  await t.expect.eventually(async () => (await linuxState(t)) !== 'loading', { message: 'the Linux block to read WSL' });
  await t.page.eval(`[...document.querySelectorAll('*')].find((e) => e.children.length === 0 && e.innerText?.trim() === 'Run agents in Linux (WSL)')?.scrollIntoView({ block: 'start', behavior: 'instant' })`);
  return linuxState(t);
}

/** What the block shows: ready (distros listed), not_installed, no_distro, broken, or loading. */
export function linuxState(t) {
  return t.page.eval(`(() => {
    if (document.querySelector('[aria-label="Linux distros"]')) return 'ready';
    const text = document.body.innerText;
    const at = text.indexOf('Run agents in Linux (WSL)');
    if (at < 0) return 'loading';
    const tail = text.slice(at, at + 600);
    if (tail.includes('WSL is not installed')) return 'not_installed';
    if (tail.includes('WSL has no Linux yet')) return 'no_distro';
    if (tail.includes('WSL did not answer as expected')) return 'broken';
    return 'loading';
  })()`);
}

export { scrollToCss };

/** The value of a "Runs on" list ('' is Windows). */
export const runsOn = (t, agent) => t.page.eval(`document.querySelector('select[aria-label="Runs on for ${agent}"]')?.value ?? null`);

/** Choose where an agent runs ('' for Windows) the way a person would with the list. */
export async function chooseRunsOn(t, agent, value) {
  await t.page.eval(`(() => {
    const sel = document.querySelector('select[aria-label="Runs on for ${agent}"]');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(sel, ${JSON.stringify(value)});
    sel.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
}
