// What broke it: the command palette has the same sheet as the History button.
import { gitProject, CHECK_MJS, paneWithFirstSavePoint } from './_area.mjs';

export const meta = {
  id: 'whatbroke.opens-from-palette',
  title: 'The command palette opens the What broke the tests sheet for the selected pane',
  area: 'whatbroke',
  tags: ['whatbroke'],
};

export async function run(t) {
  const cwd = gitProject(t, 'palette', { 'check.mjs': CHECK_MJS });
  await paneWithFirstSavePoint(t, cwd);

  await t.step('Open the command palette with Ctrl+K and type "broke"');
  await t.press('Ctrl+K');
  await t.type('broke');
  await t.expect.text('Find what broke the tests');
  await t.expect.text("Look back through the agents' turns.");

  await t.step('Choose it: the sheet opens with its test command box');
  await t.click('Find what broke the tests');
  await t.waitFor('Test command');
  await t.expect.text('Puppet Master runs your test command on older save points');
  await t.expect.text('None yet');
}
