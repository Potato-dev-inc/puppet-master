// A Claude Code agent hits its usage limit and says it will continue by itself later. Its written work stays, the app shows the wait
// (not an error), and the app does not type anything to hurry it.
import { calm, gitProject, groupOfRow, fs, path } from './_area.mjs';
import { installFakeProgram } from '../night/_area.mjs';

export const meta = {
  id: 'work.long-usage-limit-auto-continue',
  title: 'A limit with "continuing automatically" keeps the written file, is not an error, and nothing is typed to hurry it',
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
  { do = "write_file", path = "progress.txt", content = "part one done\\n" },
  { do = "say", text = "Part one is finished." },
  { do = "limit", reset = "5pm", auto_continue = true },
]
`;

export async function run(t) {
  await calm(t);
  const undo = installFakeProgram(t, 'claude');
  try {
    const cwd = gitProject(t, 'autolimit', { 'README.md': 'demo\n' });
    t.fake.script(SCRIPT);
    const pane = await t.spawnPane('claude', { cwd, waitFor: /❯/, idle: false });
    await t.expect.eventually(async () => t.fake.events().some((e) => e.ev === 'ready'), { message: 'the fake Claude is ready' });
    await t.sleep(1500);

    await t.step('The agent writes part one, then hits the limit and says it will continue by itself');
    await pane.write('do both parts');
    await t.fake.waitEvent('limit', { timeout: 30000 });
    await pane.waitScreen('continuing automatically', { timeout: 20000 });
    t.expect.equal(fs.readFileSync(path.join(cwd, 'progress.txt'), 'utf8'), 'part one done\n');

    await t.step('The pane is still there and is not reported as an error');
    await t.sleep(3000);
    const info = await pane.info();
    t.expect.ok(info, 'the pane is still listed');
    t.expect.ok(info.status !== 'error', `the status is ${info.status}, not an error`);

    await t.step('Nothing was typed into the agent by the app');
    const events = t.fake.events();
    const afterLimit = events.slice(events.findIndex((e) => e.ev === 'limit') + 1);
    t.expect.equal(afterLimit.filter((e) => ['key', 'prompt'].includes(e.ev)).length, 0, 'no keys or prompts after the limit');
    // leave no limit card behind for the next test
    if (await t.page.has('exact=Dismiss')) await t.click('exact=Dismiss');
    void groupOfRow;
  } finally {
    undo();
  }
}
