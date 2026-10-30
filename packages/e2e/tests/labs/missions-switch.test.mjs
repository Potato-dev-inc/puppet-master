// Labs > Missions: the switch adds "Start a mission…" to the plus menu, which opens the New mission sheet; off removes it.
import { flip, gitProject, isOn, labsOff, openNewMenu, openSettings, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'labs.missions-switch',
  title: 'The Missions switch adds Start a mission to the plus menu and the sheet opens',
  area: 'labs',
  tags: ['labs'],
};

export async function run(t) {
  try {
    await t.spawnPane('nimbus', { cwd: gitProject(t, 'mission-app') });

    await t.step('The plus menu has no mission item while the switch is off');
    await openNewMenu(t);
    await t.expect.notVisible('Start a mission');
    await t.press('Escape');

    await t.step('Turn Missions on in Settings');
    await openSettings(t, 'General');
    await flip(t, 'Missions (Preview)');
    await t.expect.eventually(() => isOn(t, 'Missions (Preview)'), { message: 'the switch turns on' });

    await t.step('The plus menu now offers a mission');
    await t.click('exact=Inbox');
    await openNewMenu(t);
    await t.expect.visible('exact=Start a mission…');

    await t.step('The New mission sheet opens');
    await sclick(t, 'exact=Start a mission…');
    await t.waitFor('exact=New mission');
    await t.expect.text('What should be true when it\'s done?');
    await t.expect.text('Agents may change tests');
    await t.expect.text('Missions are new. Every agent works in a visible pane so you can watch.');

    await t.step('Cancel closes it and starts nothing');
    await t.click('exact=Cancel');
    await t.expect.notVisible('exact=New mission');
    t.expect.equal((await t.panes()).length, 1, 'only the pane the test started is open');
  } finally {
    await labsOff(t);
  }
}

export const setup = labsOff;
