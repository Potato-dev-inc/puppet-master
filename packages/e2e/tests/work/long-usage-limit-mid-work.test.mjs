// A Claude Code agent runs out of usage in the middle of its work. The file it wrote stays, the Inbox says so, and nothing is typed by itself.
import { calm, gitProject, waitStatus, fs, path } from './_area.mjs';
import { installFakeProgram } from '../night/_area.mjs';

export const meta = {
  id: 'work.long-usage-limit-mid-work',
  title: 'A usage limit hit after some work keeps the written file, shows a card and types nothing by itself',
  area: 'work',
  tags: ['work', 'long', 'limit'],
  edge: true,
  timeout: 120000,
};

const SCRIPT = `agent = "claude"
boot_ms = 100
[[turn]]
steps = [
  { do = "think", ms = 1200 },
  { do = "write_file", path = "progress.txt", content = "half way there\\n" },
  { do = "say", text = "Half way through the job." },
  { do = "limit", text = "⎿  You've hit your session limit · resets 5pm", reset = "5pm" },
]
`;

export async function run(t) {
  await calm(t);
  const undo = installFakeProgram(t, 'claude');
  try {
    const cwd = gitProject(t, 'limit', { 'README.md': 'demo\n' });
    t.fake.script(SCRIPT);
    const pane = await t.spawnPane('claude', { cwd, waitFor: /❯/, idle: false });
    await t.expect.eventually(async () => t.fake.events().some((e) => e.ev === 'ready'), { message: 'the fake Claude is ready' });
    await t.sleep(1500);

    await t.step('The agent writes a file, says it is half way, then hits its limit');
    await pane.write('please do the whole job');
    await t.fake.waitEvent('limit', { timeout: 30000 });
    t.expect.equal(fs.readFileSync(path.join(cwd, 'progress.txt'), 'utf8'), 'half way there\n');

    await t.step('The Inbox says Claude Code ran out of usage and offers choices');
    await t.expect.text('Claude Code ran out of usage.', { timeout: 30000 });
    await t.expect.visible('exact=Dismiss');

    await t.step('Nothing was typed into the agent after the limit, and the pane is still there');
    const events = t.fake.events();
    const afterLimit = events.slice(events.findIndex((e) => e.ev === 'limit') + 1);
    t.expect.equal(afterLimit.filter((e) => ['key', 'prompt'].includes(e.ev)).length, 0, 'nothing typed after the limit');
    t.expect.equal((await t.panes()).length, 1);

    await t.step('Dismiss the card: the pane keeps its work');
    await t.click('exact=Dismiss');
    await t.expect.noText('Claude Code ran out of usage.');
    t.expect.ok(fs.existsSync(path.join(cwd, 'progress.txt')));
  } finally {
    undo();
  }
}
