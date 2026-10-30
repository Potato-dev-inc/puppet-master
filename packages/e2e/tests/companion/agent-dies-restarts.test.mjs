// A companion whose agent keeps quitting is started again by the keeper (after a pause), and says so in its feed.
import { noCompanions, invoke, draft, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.agent-dies-restarts',
  title: "A companion whose agent quits is started again and the feed says so",
  area: 'companion',
  tags: [],
  edge: true,
  timeout: 90000,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  t.fake.script(`
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "exit", code = 3 },
]
`);
  await t.step('Wake a companion whose agent quits as soon as it gets a message');
  const made = await invoke(t, 'companion_create', { input: draft() });
  t.expect.ok(made.ok, 'made');
  const id = made.ok.id;

  await t.step('The first process ends');
  await t.fake.waitEvent('exit');

  await t.step('After a short pause the app starts it again');
  await t.expect.eventually(async () => {
    const feed = (await invoke(t, 'companion_feed', { id, sinceMs: 0 })).ok || [];
    return feed.some((f) => f.kind === 'restarted' && f.text.includes('Restarted after it stopped.'));
  }, { timeout: 45000, message: 'expected a "Restarted after it stopped." line in its feed' });
  t.expect.ok(t.fake.events().filter((e) => e.ev === 'start').length >= 2, 'the agent was started a second time');
  t.expect.ok((await companionList(t))[0].restarts >= 1, 'the restart is counted');
});
