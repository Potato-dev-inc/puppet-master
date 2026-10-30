// KNOWN BUG: for a numbered-menu question the id that wait_agents reports is not the id answer_prompt accepts.
import { mcp, firstRun, guarded, waitBlocked } from '../companion/_area.mjs';

export const meta = {
  id: 'runs.answer-menu-prompt-by-id',
  title: 'A numbered-menu question can be answered with the id the run view gives',
  area: 'runs',
  tags: ['mcp', 'known-bug'],
  edge: true,
  skip: 'wait_agents shows prompt id A for a numbered menu, answer_prompt demands id B (STALE_PROMPT); the [y/n] question is fine',
};

const MENU_SCRIPT = `
boot_ms = 100
inline = true
approvals = "menus"
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "echo hi", on_allow = [{ do = "say", text = "the command ran" }], on_deny = [{ do = "say", text = "the command was refused" }] },
]
`;

export const run = guarded(async (t) => {
  if (meta.skip) t.skip(meta.skip);
  t.fake.script(MENU_SCRIPT);
  const project = t.project('menu');
  const m = mcp(t);
  const handle = (await m.run({ agent_type: 'nimbus', project_path: project, task: 'ask me with a menu' })).body.handle;
  const blocked = await waitBlocked(t, m, project, handle);

  await t.step('Answer "Allow once" with the id from the run view');
  const a = await m.answer({ project_path: project, handle, prompt_id: blocked.prompt.id, choice: blocked.prompt.options[0].id });
  t.expect.equal(a.status, 200, `answer_prompt said ${JSON.stringify(a.body).slice(0, 300)}`);

  await t.step('The run carries on');
  t.expect.equal(firstRun(await m.wait(project, handle)).status, 'completed');
});
