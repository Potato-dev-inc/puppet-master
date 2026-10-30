// A mission needs words and at least one check. Start stays off and says what is missing; nothing is started here.
import { blockedLine, gitProject, isDisabled, labsOff, pickPane, setSettings } from '../night/_area.mjs';

export const meta = {
  id: 'labs.missions-sheet-needs-a-check',
  title: 'The New mission sheet keeps Start off until there is a goal and a check',
  area: 'labs',
  tags: ['labs'],
  edge: true,
};

const blocked = (t) => t.page.eval(`document.querySelector('.ck-mission-blocked')?.innerText ?? ''`);

export async function run(t) {
  try {
    await t.spawnPane('nimbus', { cwd: gitProject(t, 'mission-app') });
    await setSettings(t, { missions_enabled: true });
    await pickPane(t, 'Nimbus');

    await t.step('Open the sheet from the command palette');
    await t.press('Ctrl+K');
    await t.waitFor('aria=Search commands');
    await t.type('mission');
    await t.click('exact=Start a mission…');
    await t.waitFor('exact=New mission');

    await t.step('Nothing written yet: Start is off and says what to write');
    t.expect.equal(await isDisabled(t, 'Start mission'), true);
    await t.expect.eventually(async () => (await blocked(t)) === 'Say what should be true when it\'s done.', { message: 'asks for the goal' });
    await t.expect.text('At least one check is needed.');

    await t.step('A goal alone is not enough: a check is needed');
    await t.fill('css=.ck-mission-goal', 'the tests pass');
    await t.expect.eventually(async () => (await blocked(t)) !== '' && (await blocked(t)) !== 'Say what should be true when it\'s done.', { message: 'the line now asks for a check' });
    t.expect.equal(await isDisabled(t, 'Start mission'), true);

    await t.step('Add a check: Start turns on (it is not pressed)');
    await t.fill('aria=Add a check', 'npm test');
    await t.press('Enter');
    await t.expect.eventually(async () => (await isDisabled(t, 'Start mission')) === false, { message: 'Start is enabled' });
    await t.expect.text('npm test');

    await t.step('Cancel leaves everything as it was');
    await t.click('exact=Cancel');
    await t.expect.notVisible('exact=New mission');
  } finally {
    await labsOff(t);
  }
}

export const setup = labsOff;
