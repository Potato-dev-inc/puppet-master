// KNOWN BUG: the Tests list of the first task opens upward and its top part (the "Found: ... Use" suggestion and the
// close button) sits above the sheet's top edge, so Use cannot be pressed: the click lands outside the sheet and closes it.
// Run with E2E_KNOWN_BUGS=1 to see the failure and record its video; otherwise the test is skipped.
import { gitProject, nightSetup, openTonight, sclick } from './_area.mjs';

export const meta = {
  id: 'night.tests-popover-clipped',
  title: 'Every button in the Tests list of the first task is inside the sheet',
  area: 'night',
  tags: ['night', 'known-bug'],
  edge: true,
};

export async function run(t) {
  if (!process.env.E2E_KNOWN_BUGS) t.skip('known bug: the Tests list of task 1 opens above the sheet and its Use and close buttons cannot be pressed (set E2E_KNOWN_BUGS=1 to run)');
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
