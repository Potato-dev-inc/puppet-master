// Performance sanity: twelve panes at once. The sidebar lists them all, each one opens quickly and the cockpit stays usable.
import { calm, openPane, PANE_TITLE, sidebarTitles, paneHeading } from './_area.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export const meta = {
  id: 'sidebar.many-panes',
  title: 'Twelve panes are all listed, open quickly and close again',
  area: 'sidebar',
  tags: ['performance'],
  edge: true,
  timeout: 150000,
};

const COUNT = 12;

export async function run(t) {
  await calm(t);

  await t.step(`Start ${COUNT} agents`);
  const panes = [];
  for (let i = 1; i <= COUNT; i++) panes.push(await t.spawnPane('nimbus', { cwd: t.project(`p${i}`) }));

  await t.step('All of them are in the sidebar');
  await t.expect.count(PANE_ROW, COUNT);
  const titles = await sidebarTitles(t);
  t.expect.equal(titles.length, COUNT, 'twelve rows');
  t.expect.equal(new Set(titles).size, COUNT, 'every name is different');
  t.expect.includes(titles, 'Nimbus 12');

  await t.step('Open the first, the middle and the last one: each opens within a few seconds');
  for (const name of ['Nimbus 1', 'Nimbus 6', 'Nimbus 12']) {
    const started = Date.now();
    await openPane(t, name);
    await t.expect.eventually(async () => (await paneHeading(t)) === name, { message: `the header says ${name}` });
    await t.expect.text('NIMBUS 0.4.2');
    const ms = Date.now() - started;
    t.log(`${name} opened in ${ms} ms`);
    t.expect.ok(ms < 8000, `${name} took ${ms} ms to open`);
  }

  await t.step('The command palette lists them all as well');
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type('Nimbus 1');
  await t.expect.text('Nimbus 12');
  await t.press('Escape');

  await t.step('Close them from outside: the sidebar empties');
  for (const p of panes) await p.kill();
  await t.expect.count(PANE_ROW, 0);
  await t.expect.text('No agents running.');
  await t.expect.notVisible(PANE_TITLE);
}
