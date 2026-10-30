// Team (Heads-up) is a Labs feature and off by default: the Inbox has no Team section, the team tools say so in plain words,
// and Settings offers to create or join a team.
import { guarded, mcp, rawBridge, openGeneralSettings, reveal } from '../companion/_area.mjs';

export const meta = {
  id: 'team.off-by-default',
  title: 'Team is off by default: no Inbox section, the team tools say so, Settings offers create or join',
  area: 'team',
  tags: ['smoke', 'needs-external'], // teammates and a relay need a network; this is the offline side
};

export const run = guarded(async (t) => {
  await t.spawnPane('nimbus'); // an app in use, like a person's
  await t.step('The Inbox has no Team section');
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.expect.text('Nothing needs you');
  await t.expect.noText('From your team');
  t.expect.equal(await t.page.count('css=section[aria-label="Team"]'), 0, 'no Team section');

  await t.step('The phone-side card list says Team is off');
  const cards = await rawBridge(t, { path: '/team/cards' });
  t.expect.equal(cards.status, 200);
  t.expect.equal(cards.body.enabled, false);
  t.expect.equal(cards.body.cards, []);

  await t.step('An agent asking for the roster over MCP gets a plain sentence, not a crash');
  const roster = await mcp(t).call('POST', '/team/agent/roster', {});
  t.expect.equal(roster.status, 409);
  t.expect.equal(roster.body.code, 'TEAM_OFF');
  t.expect.includes(roster.body.message, 'The person turns it on in Settings > General > Labs > Team.');

  await t.step('Settings > General > Labs > Team offers to create a team or join one');
  await openGeneralSettings(t);
  await reveal(t, '.st-team');
  await t.expect.text('Create a team');
  await t.expect.text('Join with an invite');
  await t.expect.text('Nothing runs or gets pushed without your click.');
});
