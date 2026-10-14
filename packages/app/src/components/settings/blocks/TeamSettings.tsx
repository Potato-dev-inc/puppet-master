// Settings › General › Labs › Team ("Heads-up"). Off by default. Points this app at a shared folder
// and names the people on the team. v0 has no invites and no encryption: anyone who can write to
// the folder can put a card in your Inbox (cards only; nothing runs on its own).
import { useEffect, useState } from 'react';
import { teamApi, type TeamApi, type TeamConfigView } from '../../../lib/team';
import { Switch } from '../controls';
import { Btn, FieldInput, Note, SettingBlock } from '../settings-ui';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));
const splitNames = (text: string) =>
  text
    .split(',')
    .map((n) => n.trim())
    .filter(Boolean);
const splitLines = (text: string) =>
  text
    .split('\n')
    .map((n) => n.trim())
    .filter(Boolean);

export function TeamSettings({ api = teamApi, onSaved }: { api?: TeamApi; onSaved?: () => void }) {
  const [view, setView] = useState<TeamConfigView | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [folder, setFolder] = useState('');
  const [name, setName] = useState('');
  const [members, setMembers] = useState('');
  const [projects, setProjects] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .configGet()
      .then((v) => {
        setView(v);
        setEnabled(v.enabled);
        setFolder(v.folder);
        setName(v.name);
        setMembers(v.members.join(', '));
        setProjects(v.projects.join('\n'));
      })
      .catch((err) => setError(errText(err)));
  }, [api]);

  const save = async (nextEnabled = enabled) => {
    setBusy(true);
    setError(null);
    try {
      const v = await api.configSet({ enabled: nextEnabled, folder, name, members: splitNames(members), projects: splitLines(projects) });
      setView(v);
      setEnabled(v.enabled);
      onSaved?.();
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingBlock
      label="Team (Labs)"
      description="When your agent pushes, offer to tell your teammates. When theirs pushes, show a card to pull it or have your agent merge it in a separate copy. Nothing runs or gets pushed without your click."
    >
      <div className="st-team">
        <div className="st-team-row">
          <span>Use Team</span>
          <Switch
            checked={enabled}
            label="Use Team"
            disabled={busy}
            onChange={(next) => {
              setEnabled(next);
              void save(next);
            }}
          />
        </div>
        <label className="st-team-row">
          <span>Your name</span>
          <FieldInput value={name} placeholder="Maya" maxLength={40} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="st-team-row">
          <span>Teammates</span>
          <FieldInput value={members} placeholder="Leo, Sam" onChange={(e) => setMembers(e.target.value)} />
        </label>
        <label className="st-team-row">
          <span>Shared folder</span>
          <FieldInput value={folder} className="font-mono" placeholder="D:\team\wire" onChange={(e) => setFolder(e.target.value)} />
        </label>
        <label className="st-team-row is-top">
          <span>Projects</span>
          <textarea
            className="st-field is-mono"
            rows={2}
            value={projects}
            placeholder="Optional: one folder per line. Folders with an open agent are watched anyway."
            onChange={(e) => setProjects(e.target.value)}
          />
        </label>
        <div className="st-team-row">
          <span />
          <Btn primary disabled={busy} onClick={() => void save()}>
            Save
          </Btn>
        </div>
        {view?.profile && <Note>This is the “{view.profile}” profile, a separate Puppet Master on this computer.</Note>}
        <Note>
          Preview: teammates share one folder (a synced or network folder). Only branch names, commits, file names and counts are shared, never code. A private relay
          with end-to-end encryption replaces the folder later.
        </Note>
        {error && <Note role="alert">{error}</Note>}
      </div>
    </SettingBlock>
  );
}
