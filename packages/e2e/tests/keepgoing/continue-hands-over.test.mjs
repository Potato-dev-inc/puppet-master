// Continue on Codex CLI: the old agent stops, a new pane starts in the same folder with a short brief, and the sidebar says
// where the task went.
import { baseSetup, sclick, waitClean } from '../night/_area.mjs';
import { CODEX_CARRIES_ON, claudeHitsItsLimit } from './_limit.mjs';

export const meta = {
  id: 'keepgoing.continue-hands-over',
  title: 'Continue on another agent hands the task over in the same folder with a brief',
  area: 'keepgoing',
  tags: ['keepgoing'],
  timeout: 120000,
};

export async function run(t) {
  await waitClean(t); // the helper pane the launcher used to open the window
  const { undo, dir } = await claudeHitsItsLimit(t, { codex: true, project: 'handover-app' });
  try {
    await t.step('Press Continue on Codex CLI');
    t.fake.script(CODEX_CARRIES_ON); // the fake that starts next plays the Codex screens
    await sclick(t, 'exact=Continue on Codex CLI');

    await t.step('The old pane says it was handed over');
    await t.expect.text('Handed to Codex CLI');

    await t.step('A Codex pane started in the same folder');
    await t.expect.eventually(async () => (await t.panes()).some((p) => p.agent_type === 'codex'), { message: 'a codex pane exists' });
    const panes = await t.panes();
    t.expect.equal(panes.find((p) => p.agent_type === 'codex').cwd.toLowerCase(), dir.toLowerCase());
    await t.expect.text('Codex CLI');

    await t.step('Its first words were the brief, and it carried on');
    const brief = await t.fake.waitEvent((e) => e.ev === 'prompt' && /^Continue:/.test(e.text || ''), { timeout: 40000 });
    t.expect.includes(brief.text, 'fenced block', 'the brief says the notes are fenced');
    t.expect.ok(!/pm-e2e/.test(brief.text), 'no folder path of the test PC in the brief');

    await t.step('The old agent was never typed at after its limit');
    const events = t.fake.events();
    const limitAt = events.findIndex((e) => e.ev === 'limit');
    const between = events.slice(limitAt + 1, events.findIndex((e, i) => i > limitAt && e.ev === 'start'));
    t.expect.equal(between.filter((e) => ['prompt', 'approval_answer', 'gate_answer'].includes(e.ev)).length, 0);
  } finally {
    undo();
  }
}

export { baseSetup as setup };
