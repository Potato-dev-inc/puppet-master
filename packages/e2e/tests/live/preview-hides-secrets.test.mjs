// "What viewers see" is the exact first snapshot: a key is replaced and the folder path is cut to its last name.
import { openGoLive, guarded } from '../companion/_area.mjs';
import { openChat, sendChat } from '../_helpers.mjs';

export const meta = {
  id: 'live.preview-hides-secrets',
  title: 'The Go live preview hides keys and folder paths the agent printed',
  area: 'live',
  tags: ['security'],
  edge: true,
};

const SCRIPT = `
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "say", text = 'my key is sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd and the folder is C:/Users/someone/secret-project' },
]
`;

export const run = guarded(async (t) => {
  t.fake.script(SCRIPT);
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'tell me where you are');
  await t.expect.text('my key is sk-ant-api03');

  await t.step('Open Go live: the preview shows what viewers would get');
  await openGoLive(t);
  await t.expect.text('What viewers see');
  await t.expect.text('my key is [REDACTED]');

  await t.step('The key is gone and only the last folder name is left');
  const preview = await t.page.eval(`document.querySelector('.ck-sheet--live').innerText`);
  t.expect.ok(!preview.includes('sk-ant'), 'no part of the key is shown');
  t.expect.ok(!preview.includes('C:/Users/someone'), 'the path above the project folder is not shown');
  t.expect.includes(preview, 'secret-project');
});
