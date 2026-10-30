// TEMPLATE: copy this file to tests/<area>/<name>.test.mjs, then change `meta` and `run`.
// Files whose name starts with "_" are never run.
//
// Rules: one behaviour per test; the test starts from an empty app (no panes); fake agents only;
// assert what a person SEES (text, buttons), not colours or class names; name the area after docs/FEATURES.md.
import { openChat, sendChat, replyScript } from './_helpers.mjs'; // from tests/<area>/ use '../_helpers.mjs'

export const meta = {
  id: 'area.what-it-does', // area.name, lowercase, dashes; the area is the first part and the folder name
  title: 'One sentence a person would say',
  area: 'area',
  tags: [], // 'smoke' for the quick set; any other labels for filtering
  edge: false, // true for an edge case
  // fresh: true, // restart the app first (settings, UI mode or other persisted state changes)
  // timeout: 60000,
};

export async function run(t) {
  // 1. Set up state. A fake agent plays a script; the bridge is available as t.bridge.
  t.fake.script(replyScript('hello from the fake agent'));
  const pane = await t.spawnPane('nimbus'); // pane.screen(), pane.write(text), pane.waitScreen(text)

  // 2. Act like a person. Every step name shows in the video.
  await t.step('Open the Chat tab');
  await openChat(t);
  await t.step('Send a message');
  await sendChat(t, 'hi');

  // 3. Assert. The page.* and expect.* calls wait for you, so there are no sleeps.
  await t.expect.text('hello from the fake agent');
  t.expect.equal((await pane.info()).agent_type, 'nimbus');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 1);
}
