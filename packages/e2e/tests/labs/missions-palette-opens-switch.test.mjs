// With Missions off, the palette still lists "Missions…" and it opens the switch in Settings, so the feature can be found.
import { isOn, labsOff, openNewMenu } from '../night/_area.mjs';

export const meta = {
  id: 'labs.missions-palette-opens-switch',
  title: 'With Missions off, the palette row opens the switch instead of a mission',
  area: 'labs',
  tags: ['labs'],
  edge: true,
};

export async function run(t) {
  await t.step('Open the command palette and look for missions');
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
  await t.type('mission');
  await t.expect.visible('exact=Missions…');
  await t.expect.text('Preview, in Labs. Turn it on to start one.');
  await t.expect.noText('Start a mission');

  await t.step('Pick it: Settings opens on the Labs switch');
  await t.click('exact=Missions…');
  await t.waitFor('css=.ck-settings-host');
  await t.expect.text('Give agents a goal and the checks it must pass.');
  t.expect.equal(await isOn(t, 'Missions (Preview)'), false, 'nothing was turned on for the person');

  await t.step('The plus menu still has no mission item');
  await t.click('exact=Inbox');
  await openNewMenu(t);
  await t.expect.notVisible('Start a mission');
}

export const setup = labsOff;
