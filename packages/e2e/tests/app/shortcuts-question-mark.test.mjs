// "?" opens the keyboard shortcuts sheet, Esc closes it. The list covers the groups of the app.
import { calm } from './_area.mjs';

export const meta = {
  id: 'app.shortcuts-question-mark',
  title: 'The ? key opens the keyboard shortcuts sheet and Esc closes it',
  area: 'app',
  tags: ['smoke', 'keyboard'],
};

export async function run(t) {
  await calm(t);
  await t.step('Press ?');
  await t.press('?');
  await t.waitFor('css=.ck-sheet--shortcuts');

  await t.step('The groups and a few keys are listed');
  for (const group of ['Navigate', 'Chat', 'Panes', 'Search']) await t.expect.visible(`exact=${group}`, { within: 'css=.ck-sheet--shortcuts' });
  await t.expect.text('Open the command palette: switch panes, start an agent, run an action');
  await t.expect.text('Show or hide the side rail');
  await t.expect.visible('exact=Ctrl+K', { within: 'css=.ck-sheet--shortcuts' });

  await t.step('Esc closes it');
  await t.press('Escape');
  await t.expect.notVisible('css=.ck-sheet--shortcuts');

  await t.step('Open again and use the Close button');
  await t.press('?');
  await t.waitFor('css=.ck-sheet--shortcuts');
  await t.click('exact=Close', { within: 'css=.ck-sheet--shortcuts' });
  await t.expect.notVisible('css=.ck-sheet--shortcuts');
}
