// Edge case: the agent leaves with exit code 0 (like typing /exit). The pane must not pretend to be running.
import { calm, openPane } from './_area.mjs';

export const meta = {
  id: 'panes.agent-quits-cleanly',
  title: 'An agent that quits with code 0 is no longer shown as working or idle',
  area: 'panes',
  tags: ['states'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  t.fake.script(`
boot_ms = 100
inline = true
[[turn]]
on_prompt = "leave now"
steps = [
  { do = "say", text = "see you" },
  { do = "exit", code = 0 },
]
`);
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');

  await t.step('Tell the agent to leave');
  await pane.write('leave now');
  await t.fake.waitEvent('exit');

  await t.step('The app notices that the program ended');
  await t.expect.eventually(async () => {
    const info = await pane.info();
    return !info || info.status === 'exited' || info.status === 'error';
  }, { message: 'the pane is no longer idle or working', timeout: 15000 });

  await t.step('The cockpit does not show it as Idle');
  const left = await pane.info();
  if (left) await t.expect.noText(/Nimbus · Idle/);
  await t.expect.text('see you');
}
