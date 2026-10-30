// Labs > Simple mode: the palette switches the whole cockpit to fewer buttons and plain words; "Switch off" in its footer
// brings the normal cockpit back.
import { labsOff, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'labs.simple-mode-switch',
  title: 'Simple mode swaps the cockpit for a plain screen and Switch off brings it back',
  area: 'labs',
  tags: ['labs'],
};

export async function run(t) {
  try {
    await t.step('The normal cockpit has the sidebar with Orchestration and Settings');
    await t.expect.visible('exact=Orchestration');

    await t.step('Switch to Simple mode from the command palette');
    await t.press('Ctrl+K');
    await t.waitFor('aria=Search commands');
    await t.type('simple');
    await t.click('exact=Switch to Simple mode');

    await t.step('The plain screen has your projects, one big button and a way back');
    await t.waitFor('exact=Make something');
    await t.expect.text('Your projects');
    await t.expect.text('You have no projects yet. Press Make something to start.');
    await t.expect.visible('exact=Switch off');
    await t.expect.notVisible('exact=Orchestration');

    await t.step('Switch off: the normal cockpit returns');
    await sclick(t, 'exact=Switch off');
    await t.waitFor('exact=Orchestration');
    await t.expect.notVisible('exact=Make something');
  } finally {
    await labsOff(t);
  }
}

export const setup = labsOff;
