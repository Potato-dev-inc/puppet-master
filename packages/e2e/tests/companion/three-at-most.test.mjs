// At most three companions: the fourth is refused until one is archived.
import { noCompanions, makeCompanion, invoke, draft, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.three-at-most',
  title: 'A fourth companion is refused until one is archived',
  area: 'companion',
  tags: [],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  await t.step('Make three companions');
  for (const [name, avatar] of [['Mochi', 'mochi'], ['Bit', 'bit'], ['Nib', 'nib']]) await makeCompanion(t, { name, avatar });
  await t.page.reload();
  await t.expect.count('css=.ck-companion-row', 3);

  await t.step('A fourth is refused with a plain sentence');
  const r = await invoke(t, 'companion_create', { input: draft({ name: 'Tofu', avatar: 'tofu' }) });
  t.expect.ok(r.err && r.err.includes('COMPANION_LIMIT'), `refused (${JSON.stringify(r)})`);
  t.expect.includes(r.err, 'Archive one first.');
  t.expect.equal((await companionList(t)).length, 3, 'still three');

  await t.step('Archive one and the fourth fits');
  await invoke(t, 'companion_archive', { id: 'nib' });
  const ok = await invoke(t, 'companion_create', { input: draft({ name: 'Tofu', avatar: 'tofu' }) });
  t.expect.ok(ok.ok, 'the fourth was made after archiving one');
});
