// General > Labs lists the early features, all off. Turning Missions on makes "Start a mission" appear in the + menu; off removes it.
import { calm, openSettings, clickControl, revealControl } from './_area.mjs';

export const meta = {
  id: 'settings.labs-missions-switch',
  title: 'The Labs list shows early features off, and the Missions switch adds Start a mission to the + menu',
  area: 'settings',
  tags: ['labs'],
};

const MISSIONS = 'aria=Missions (Preview)';
const switchOn = (t) => t.page.eval(`document.querySelector('[role=switch][aria-label="Missions (Preview)"]')?.getAttribute('aria-checked')`);

export async function run(t) {
  await calm(t);
  try {
    await openSettings(t, 'General');

    await t.step('Labs lists the early features');
    await t.expect.text('Early features');
    await t.expect.text('Each one is off until you turn it on, and may change.');
    for (const name of ['Team', 'Ask bar', 'Simple mode', 'Live updates', 'Missions', 'Night Shift']) await t.expect.text(name);

    await t.step('The Missions switch is off, and so is the menu item');
    await revealControl(t, MISSIONS);
    t.expect.equal(await switchOn(t), 'false', 'Missions is off');
    await t.click('aria=New agent');
    await t.waitFor('css=.ck-menu');
    await t.expect.notVisible('Start a mission', { within: 'css=.ck-menu' });
    await t.click('aria=New agent'); // the button closes the menu again (Esc would also leave Settings)

    await t.step('Turn Missions on');
    await clickControl(t, MISSIONS);
    await t.expect.eventually(async () => (await switchOn(t)) === 'true', { message: 'the switch is on' });

    await t.step('The + menu now offers Start a mission');
    await t.click('aria=New agent');
    await t.expect.visible('Start a mission', { within: 'css=.ck-menu' });
    await t.click('aria=New agent'); // the button closes the menu again (Esc would also leave Settings)

    await t.step('Turn it off again');
    await clickControl(t, MISSIONS);
    await t.expect.eventually(async () => (await switchOn(t)) === 'false', { message: 'the switch is off' });
    await t.click('aria=New agent');
    await t.waitFor('css=.ck-menu');
    await t.expect.notVisible('Start a mission', { within: 'css=.ck-menu' });
    await t.click('aria=New agent'); // the button closes the menu again (Esc would also leave Settings)
  } finally {
    try {
      if ((await switchOn(t)) === 'true') await clickControl(t, MISSIONS);
    } catch { /* Settings is not open */ }
  }
}
