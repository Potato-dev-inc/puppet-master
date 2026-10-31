// Scores on your code: Close leaves the page and the pane is back.
import { openSidebarPage, projectDir } from './_area.mjs';

export const meta = {
  id: 'scores.close-returns',
  title: 'Close leaves the Scores page and the selected pane is shown again',
  area: 'scores',
  tags: ['scores'],
};

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: projectDir(t, 'site') });
  await t.click('css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)', { nth: 0 });
  await t.expect.visible('exact=Terminal');

  await t.step('Open Scores');
  await openSidebarPage(t, 'Scores');
  await t.expect.text('Scores on your code');
  await t.expect.notVisible('exact=Terminal');

  await t.step('Press Close: the pane tabs are back');
  await t.click('exact=Close');
  await t.expect.visible('exact=Terminal');
  await t.expect.noText('Scores on your code');
}
