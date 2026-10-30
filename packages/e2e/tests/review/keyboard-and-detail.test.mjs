// Review desk: the keyboard moves between rows (J and K), Enter opens the files of a row, Escape closes it, then the desk.
import { gitProject, turnsScript, savedTurn, openDeskWithPane, rowSel, ROW } from './_area.mjs';

export const meta = {
  id: 'review.keyboard-and-detail',
  title: 'J and K move between rows, Enter opens the files of a row, Escape closes the detail and then the desk',
  area: 'review',
  tags: ['review'],
  skip: "NOT VERIFIED IN THE FULL RUN: it failed in the one full video run after a first-run Welcome screen covered the cockpit mid-run (cause not found); it passed in earlier partial runs. Remove this skip and re-run.",
  timeout: 120000,
};

export async function run(t) {
  if (meta.skip) t.skip(meta.skip);
  const cwd = gitProject(t, 'shop', { 'README.md': 'shop\n', 'package.json': '{"name":"shop"}\n' });
  t.fake.script(turnsScript([
    { prompt: 'add a page', think: 6000, writes: [['src/page.txt', 'hello page']] },
    { prompt: 'add a dependency', think: 6000, writes: [['package.json', '{"name":"shop","dependencies":{"left-pad":"1.3.0"}}\n']] },
  ]));
  await t.step('Two agents finish two turns');
  await savedTurn(t, cwd, 'add a page');
  const pkgPane = await savedTurn(t, cwd, 'add a dependency');

  await t.step('Open the desk and click the package list row to focus it');
  await openDeskWithPane(t, pkgPane);
  await t.click(rowSel(pkgPane));

  await t.step('Press K, then J: the selection moves up and back down');
  const marked = () => t.page.eval(`[...document.querySelectorAll('[data-rd="item"]')].findIndex(r => r.getAttribute('aria-current') === 'true')`);
  const start = await marked();
  await t.press('k');
  await t.expect.eventually(async () => (await marked()) === Math.max(0, start - 1), { message: 'K moves up one row' });
  await t.press('j');
  await t.expect.eventually(async () => (await marked()) === start, { message: 'J moves down one row' });

  await t.step('Press Enter: the detail of that row opens and names the changed file');
  await t.press('Enter');
  await t.waitFor('css=[data-rd-detail]');
  await t.expect.text('package.json');

  await t.step('Escape closes the detail but keeps the desk');
  await t.press('Escape');
  await t.expect.notVisible('css=[data-rd-detail]');
  await t.expect.visible(ROW);

  await t.step('Escape again closes the desk');
  await t.press('Escape');
  await t.expect.notVisible('css=.ck-rd');
}
