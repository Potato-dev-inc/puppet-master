// Team switched on but no team made yet: the Inbox says to finish setting it up, team cards do not count as "needs you",
// and the team tools say this computer is not on a team.
import { guarded, invoke, mcp, rawBridge } from '../companion/_area.mjs';

export const meta = {
  id: 'team.enabled-without-team',
  title: 'With Team on but no team yet the Inbox says to finish setting it up and nothing needs you',
  area: 'team',
  tags: ['needs-external'],
  edge: true,
};

const teamConfig = (enabled) => ({ config: { enabled, projects: [] } });

export const run = guarded(async (t) => {
  try {
    await t.spawnPane('nimbus'); // an app in use, like a person's
    await t.step('Switch Team on (the Labs switch) without creating a team');
    const on = await invoke(t, 'team_config_set', teamConfig(true));
    t.expect.ok(on.ok, `Team is on (${JSON.stringify(on)})`);
    await t.page.reload();

    await t.step('The Inbox shows a Team section that says to finish setting it up');
    await t.click('exact=Inbox', { within: 'css=.ck-side' });
    await t.expect.text('Team · finish setting it up in Settings › General');

    await t.step('It does not count as something that needs you');
    await t.expect.text('Nothing needs you');

    await t.step('The team tools say this computer is not on a team');
    const roster = await mcp(t).call('POST', '/team/agent/roster', {});
    t.expect.equal(roster.status, 409);
    t.expect.equal(roster.body.code, 'NO_TEAM');

    await t.step('A remote caller cannot read the team cards without a paired phone');
    const remote = await rawBridge(t, { path: '/team/cards', headers: { 'X-Forwarded-For': '203.0.113.9' } });
    t.expect.equal(remote.status, 401);
    const own = await rawBridge(t, { path: '/team/cards' });
    t.expect.equal(own.body.enabled, true);
    t.expect.equal(own.body.active, false);
  } finally {
    await invoke(t, 'team_config_set', teamConfig(false));
  }
});
