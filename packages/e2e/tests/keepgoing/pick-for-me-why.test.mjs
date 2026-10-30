// "Why this one" lists every agent with its own line and says the numbers stay on this computer.
import { openNewMenu, sclick, waitClean } from '../night/_area.mjs';

export const meta = {
  id: 'keepgoing.pick-for-me-why',
  title: 'Why this one lists each agent and says nothing you type is sent anywhere',
  area: 'keepgoing',
  tags: ['pick-for-me'],
};

export async function run(t) {
  await waitClean(t); // the helper pane the launcher used to open the window
  await openNewMenu(t);
  await sclick(t, 'Pick for me');
  await t.waitFor('css=.ck-pick');
  await t.expect.visible('exact=Why this one');

  await t.step('The details are closed at first');
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-pick-why').open`), false);

  await t.step('Open Why this one');
  await t.click('exact=Why this one');
  await t.expect.eventually(() => t.page.eval(`document.querySelector('.ck-pick-why').open`), { message: 'the details open' });

  await t.step('One line per agent, with no history yet');
  const lines = await t.page.eval(`[...document.querySelectorAll('.ck-pick-line')].map((l) => l.innerText.replace(/\\s+/g, ' ').trim())`);
  t.expect.ok(lines.length >= 2, `a line for each agent (${lines.length})`);
  t.expect.ok(lines.some((l) => l.startsWith('Nimbus') && l.includes('No runs here yet')), 'Nimbus has no runs here yet');

  await t.step('The notes say where it is worked out');
  await t.expect.text('Counted from your own turns in this project, last 90 days.');
  await t.expect.text('It looks at the whole project, not at what you type.');
  await t.expect.text('Worked out on your computer. Nothing you type is sent anywhere.');
}
