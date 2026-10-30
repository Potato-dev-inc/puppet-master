// The magnifier at the top of the sidebar opens the same palette as Ctrl+K.
import { calm } from './_area.mjs';

export const meta = {
  id: 'sidebar.search-button-opens-palette',
  title: 'The magnifier in the sidebar opens the command palette',
  area: 'sidebar',
  tags: ['palette'],
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');

  await t.step('Click the magnifier');
  await t.click('aria=Search', { within: 'css=.ck-side-head' });
  await t.waitFor('aria=Search commands');

  await t.step('The panes are listed and typing filters');
  await t.expect.text('Panes');
  await t.type('nimbus');
  await t.expect.text('New Nimbus');
  await t.press('Escape');
  await t.expect.notVisible('aria=Search commands');
}
