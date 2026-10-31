// The Tests list of the first task opens under its row, inside the sheet, so Use and the close button can be pressed.
import { gitProject, nightSetup, openTonight, sclick } from './_area.mjs';

export const meta = {
  id: 'night.tests-popover-clipped',
  title: 'Every button in the Tests list of the first task is inside the sheet',
  area: 'night',
  tags: ['night'],
  edge: true,
};

export async function run(t) {
  const dir = gitProject(t, 'suggest-app', { 'package.json': JSON.stringify({ name: 'x', scripts: { test: 'node check.js' } }) });
  await t.spawnPane('nimbus', { cwd: dir });
  await openTonight(t);

  await t.step('Open the Tests list of the first task');
  await sclick(t, 'aria=Tests for task 1');
  await t.waitFor('css=[role=dialog][aria-label=Tests]');

  await t.step('Every button of the list is inside the sheet');
  const outside = await t.page.eval(`(() => {
    const sheet = document.querySelector('.ck-night-sheet').getBoundingClientRect();
    return [...document.querySelectorAll('[role=dialog][aria-label=Tests] button')]
      .map((b) => ({ text: b.innerText.trim() || b.getAttribute('aria-label'), y: Math.round(b.getBoundingClientRect().y) }))
      .filter((b) => b.y < sheet.top);
  })()`);
  t.expect.equal(outside, [], 'buttons above the top edge of the sheet');
}

export const setup = nightSetup;
