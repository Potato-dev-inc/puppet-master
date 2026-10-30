// A companion's home must be a folder of its own: its brief is read by every agent working below it. The app refuses the
// Desktop, Documents, Downloads, the user home, the top of a drive and the Puppet Master folder, with COMPANION_FOLDER_BROAD.
import fs from 'node:fs';
import path from 'node:path';
import { noCompanions, invoke, draft, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.broad-folder-refused',
  title: 'A companion cannot live on the Desktop, Documents, the home folder or a drive root',
  area: 'companion',
  tags: ['security'],
  edge: true,
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  const home = t.app.dirs.home;
  const broad = {
    'the home folder': home,
    'the Desktop': path.join(home, 'Desktop'),
    'Documents': path.join(home, 'Documents'),
    'Downloads': path.join(home, 'Downloads'),
    'the Puppet Master folder': path.join(home, 'PuppetMaster', 'companions'),
    'the top of a drive': path.parse(home).root,
  };
  for (const dir of Object.values(broad)) if (dir !== path.parse(home).root) fs.mkdirSync(dir, { recursive: true });

  for (const [name, dir] of Object.entries(broad)) {
    await t.step(`Ask for ${name} as the home`);
    const r = await invoke(t, 'companion_create', { input: draft({ name: 'Mochi', folder: dir }) });
    t.log(name, r.err);
    t.expect.ok(r.err && r.err.includes('COMPANION_FOLDER_BROAD'), `${name} is refused with COMPANION_FOLDER_BROAD (got ${JSON.stringify(r)})`);
    t.expect.includes(r.err, 'Pick a folder of its own.', 'the refusal says what to do');
  }

  await t.step('Nothing was made and nothing was written into those folders');
  t.expect.equal((await companionList(t)).length, 0, 'no companion');
  for (const dir of Object.values(broad)) {
    if (dir === path.parse(home).root) continue;
    t.expect.ok(!fs.existsSync(path.join(dir, 'SOUL.md')), `no SOUL.md in ${path.basename(dir)}`);
    t.expect.ok(!fs.existsSync(path.join(dir, '.puppet-master')), `no marker in ${path.basename(dir)}`);
  }
});
