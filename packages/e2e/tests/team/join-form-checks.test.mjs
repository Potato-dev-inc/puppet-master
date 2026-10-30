// "Join with an invite": the code is shown as ABCD-EFGH while typing, the button waits for a link, a full code and a name,
// and a link that is not an invite is refused with a sentence and changes nothing.
import { guarded, openGeneralSettings, reveal, invoke } from '../companion/_area.mjs';

export const meta = {
  id: 'team.join-form-checks',
  title: 'The invite code is shaped as you type and a link that is no invite is refused',
  area: 'team',
  tags: ['needs-external'],
  edge: true,
};

const off = (t) => t.page.eval(`[...document.querySelectorAll('.st-team button')].find(b => b.textContent === 'Ask to join').disabled`);
const FIELD = (placeholder) => `css=.st-team input[placeholder="${placeholder}"]`;

export const run = guarded(async (t) => {
  await openGeneralSettings(t);
  await reveal(t, '.st-team');
  await reveal(t, 'input[placeholder="ABCD-EFGH"]');

  await t.step('Type a code in lower case with a space: it becomes ABCD-EFGH');
  await t.page.fill(FIELD('ABCD-EFGH'), 'abcd efgh');
  await t.expect.value(FIELD('ABCD-EFGH'), 'ABCD-EFGH');

  await t.step('More than 8 letters and digits are cut');
  await t.page.fill(FIELD('ABCD-EFGH'), 'abcdefgh1234');
  await t.expect.value(FIELD('ABCD-EFGH'), 'ABCD-EFGH');

  await t.step('The button waits for the link and your name too');
  t.expect.equal(await off(t), true);
  await t.page.fill(FIELD('pm://team/join?…'), 'https://example.com/not-an-invite');
  t.expect.equal(await off(t), true, 'no name yet');
  await t.page.fill(FIELD('Leo'), 'Leo');
  t.expect.equal(await off(t), false);

  await t.step('Ask to join with a link that is not an invite');
  await t.click('exact=Ask to join');

  await t.step('A sentence says so and no team was made');
  await t.expect.eventually(async () => (await t.page.count('css=.st-team [role="alert"]')) > 0, { message: 'expected an error line' });
  const state = (await invoke(t, 'team_config_get')).ok;
  t.expect.equal(state.team, '', 'no team');
  t.expect.equal(state.joining, null, 'no request is waiting for an admin');
});
