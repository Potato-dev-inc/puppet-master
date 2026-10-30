// Settings > Agents > Preload agents is off by default and says so; an agent with no warm mode is listed as such.
import { calm, openSettings } from './_area.mjs';

export const meta = {
  id: 'settings.preload-is-off',
  title: 'Preload agents is off by default and Nimbus has no warm mode',
  area: 'settings',
  tags: ['agents'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await openSettings(t, 'Agents');

  await t.step('Preload agents is a switch that is off');
  await t.expect.text('Preload agents');
  await t.expect.text('Off by default.');
  await t.expect.text('Off: no agents are kept warm.');
  t.expect.equal(await t.page.eval(`[...document.querySelectorAll('.st-main [role=switch]')].find((s) => /preload/i.test(s.closest('.st-row')?.innerText || ''))?.getAttribute('aria-checked')`), 'false', 'the Preload switch is off');

  await t.step('Nimbus cannot be kept warm');
  await t.expect.text('(no warm mode yet)');
  await t.expect.text('Idle timeout');
}
