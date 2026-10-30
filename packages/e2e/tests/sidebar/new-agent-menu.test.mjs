// The + button opens the New agent menu: agents, the project chip and the Separate copy switch. Esc or a click outside closes it.
import { calm } from './_area.mjs';

export const meta = {
  id: 'sidebar.new-agent-menu',
  title: 'The + button opens the New agent menu and Esc or a click outside closes it',
  area: 'sidebar',
  tags: ['smoke', 'spawn'],
};

export async function run(t) {
  await calm(t);

  await t.step('Click + (New agent)');
  await t.click('aria=New agent');
  await t.waitFor('css=.ck-menu');
  await t.expect.text('Pick for me');
  for (const agent of ['Claude Code', 'Codex CLI', 'PowerShell', 'Nimbus']) await t.expect.visible(`exact=${agent}`, { within: 'css=.ck-menu' });

  await t.step('A folder with no Git cannot use a separate copy');
  await t.expect.visible('Separate copy', { within: 'css=.ck-menu' });
  await t.expect.text('Needs Git.');
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-menu [role=switch]')?.disabled`), true, 'the Separate copy switch is off and disabled');

  await t.step('Esc closes the menu');
  await t.press('Escape');
  await t.expect.notVisible('css=.ck-menu');

  await t.step('Open it again and click outside');
  await t.click('aria=New agent');
  await t.waitFor('css=.ck-menu');
  await t.click([700, 500]);
  await t.expect.notVisible('css=.ck-menu');
  t.expect.equal((await t.panes()).length, 0, 'nothing was started');
}
