// An update menu on the companion's own pane is a notice, not a question: the app closes it with the agent's own
// "skip" key (Esc) and the companion becomes ready. This needs the Codex layout, so a copy of the fake agent is put on
// the app's program path under the name `codex` for this test.
import fs from 'node:fs';
import path from 'node:path';
import { noCompanions, invoke, draft, companionList, guarded } from './_area.mjs';
import { fakeTuiExe, copyFakeTui } from '../../src/paths.mjs';

export const meta = {
  id: 'companion.startup-notice-dismissed',
  title: "A dismissible update notice on the companion's pane is closed by the app and it becomes ready",
  area: 'companion',
  tags: ['startup-gate'],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const codex = path.join(t.app.dirs.bin, 'codex.exe');
  copyFakeTui(codex);
  try {
    t.fake.script(`
agent = "codex"
boot_ms = 100
version_output = "codex-cli 0.162.1"
[[startup]]
gate = "update_menu"
after_ms = 600
`);
    await t.step('Wake a companion on an agent that opens an update menu at start');
    const made = await invoke(t, 'companion_create', { input: draft({ agentType: 'codex' }) });
    t.expect.ok(made.ok, `the companion was made (${JSON.stringify(made)})`);
    const id = made.ok.id;

    await t.step('The menu is shown, then closed with the key the agent declares for it');
    await t.fake.waitEvent('gate_shown');
    await t.fake.waitEvent('gate_answer');
    t.expect.ok(t.fake.events().some((e) => e.ev === 'key' && /esc/i.test(String(e.key))), 'Esc, the key the update menu declares for "skip", was pressed');

    await t.step('The companion is ready and never asked the person');
    await t.expect.eventually(async () => (await companionList(t)).find((c) => c.id === id)?.state === 'idle', { timeout: 40000, message: 'expected the companion to be idle' });
    await t.page.reload();
    await t.click('exact=Mochi', { within: 'css=.ck-side' });
    await t.expect.text('Here. Ask me anything.');
    t.expect.equal((await companionList(t))[0].needs, null, 'no card was raised');
  } finally {
    try { fs.unlinkSync(codex); } catch { /* the app may still hold it; it is in the run's temp folder */ }
  }
});
