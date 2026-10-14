// Settings › General › Labs › Team ("Heads-up"). Off by default. Create a team or join one with an invite;
// the admin lets people in and removes them. Messages are end-to-end encrypted; the relay (or the shared
// folder) only moves ciphertext. No ids are shown here: people are names, the team is its name.
import { useEffect, useState } from 'react';
import { tauri } from '../../../lib/tauri';
import { teamApi, type MemberView, type TeamApi, type TeamConfigView, type TeamCreateInput, type TeamJoinInput } from '../../../lib/team';
import { ConfirmSheet, Switch } from '../controls';
import { Btn, FieldInput, Note, SettingBlock } from '../settings-ui';
import { InviteSheet } from './InviteSheet';

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));
const splitLines = (text: string) =>
  text
    .split('\n')
    .map((n) => n.trim())
    .filter(Boolean);

/** "abcd efgh" or "ABCD-EFGH" -> "ABCD-EFGH" while typing (letters and digits only, 8 at most). */
function formatCode(text: string): string {
  const raw = text
    .replace(/[^a-z0-9]/gi, '')
    .toUpperCase()
    .slice(0, 8);
  return raw.length > 4 ? `${raw.slice(0, 4)}-${raw.slice(4)}` : raw;
}

/** Where people learn to host their own relay (Cloudflare in a few clicks, or Docker). */
export const RELAY_HELP_URL = 'https://github.com/Potato-dev-inc/puppet-master/blob/main/relay/README.md';

function RelayHelp() {
  return (
    <div className="st-team-row">
      <span />
      <p className="st-note st-team-relay-help">
        Use Puppet Master&apos;s relay, or host your own (Cloudflare or Docker).{' '}
        <a
          className="st-link"
          href={RELAY_HELP_URL}
          target="_blank"
          rel="noreferrer"
          onClick={(e) => {
            // In the desktop app the system browser opens it; elsewhere the link works as usual.
            if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
              e.preventDefault();
              void tauri.openExternalUrl(RELAY_HELP_URL);
            }
          }}
        >
          How to host your own
        </a>
      </p>
    </div>
  );
}

function roleWords(m: MemberView): string {
  if (m.admin || m.roles.includes('admin')) return 'Admin';
  return m.roles.includes('approve') ? 'Can answer' : 'Watching';
}

function githubText(login: string | null, confirmed: boolean): string | null {
  return login ? `GitHub: ${login}${confirmed ? '' : ' (claimed)'}` : null;
}

function connectionText(view: TeamConfigView): string | null {
  if (view.transport === 'folder') return 'Shared folder';
  if (view.transport !== 'relay') return null;
  if (view.link === 'live') return 'Relay · live';
  if (view.link === 'polling') return 'Relay · polling';
  if (view.link === 'offline') return "Relay · can't reach it";
  return 'Relay';
}

export function TeamSettings({ api = teamApi, onSaved }: { api?: TeamApi; onSaved?: () => void }) {
  const [view, setView] = useState<TeamConfigView | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [projects, setProjects] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [inviting, setInviting] = useState(false);
  const [removing, setRemoving] = useState<MemberView | null>(null);
  const [leaving, setLeaving] = useState(false);

  useEffect(() => {
    api
      .configGet()
      .then((v) => {
        setView(v);
        setEnabled(v.enabled);
        setProjects(v.projects.join('\n'));
      })
      .catch((err) => setError(errText(err)));
  }, [api]);

  /** Run a call that returns the new settings view. */
  const run = async (what: () => Promise<TeamConfigView>, saved = false): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      const v = await what();
      setView(v);
      setEnabled(v.enabled);
      if (saved) onSaved?.();
      return true;
    } catch (err) {
      setError(errText(err));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const save = (nextEnabled = enabled) => run(() => api.configSet({ enabled: nextEnabled, projects: splitLines(projects) }), true);

  const body = () => {
    if (!view) return error ? null : <Note>Loading…</Note>;
    if (view.joining) {
      return (
        <div className="st-team-section">
          <p className="st-team-line">Waiting for {view.joining}'s admin to let you in.</p>
          <div>
            <Btn disabled={busy} onClick={() => void run(() => api.leave())}>
              Cancel
            </Btn>
          </div>
        </div>
      );
    }
    if (view.team === '') {
      return (
        <>
          <CreateForm busy={busy} onCreate={(input) => run(() => api.create(input), true)} />
          <JoinForm busy={busy} onJoin={(input) => run(() => api.join(input), true)} />
        </>
      );
    }
    const connection = connectionText(view);
    return (
      <>
        <div className="st-team-head">
          <strong>{view.teamName}</strong>
          <span>
            {' · '}
            {view.admin ? "You're the admin" : `${view.members.length} ${view.members.length === 1 ? 'person' : 'people'}`}
          </span>
        </div>
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
        {connection && <p className="st-team-line">{connection}</p>}
        <Note>
          Keys are kept in {view.keyStore}. Messages are end-to-end encrypted; the relay sees only who sends when, never names, branches or code.
        </Note>

        <ul className="st-team-people" aria-label="People">
          {view.members.map((m) => {
            const gh = githubText(m.github, m.githubConfirmed);
            return (
              <li key={m.id} className="st-team-member">
                <div className="st-team-who">
                  <span>
                    {m.name}
                    {m.me && <span className="st-team-you"> (you)</span>}
                  </span>
                  {gh && <small>{gh}</small>}
                </div>
                <span className="st-team-role">{roleWords(m)}</span>
                <span className="st-team-slot">
                  {view.admin && !m.me && (
                    <Btn small disabled={busy} onClick={() => setRemoving(m)}>
                      Remove
                    </Btn>
                  )}
                </span>
              </li>
            );
          })}
        </ul>

        {view.admin && view.joins.length > 0 && (
          <ul className="st-team-people" aria-label="Waiting to join">
            {view.joins.map((j) => (
              <li key={j.id} className="st-team-member">
                <div className="st-team-who">
                  <span>{j.name} wants to join</span>
                  <small>{[githubText(j.github, false), j.device].filter(Boolean).join(' · ')}</small>
                </div>
                <Btn small primary disabled={busy} onClick={() => void run(() => api.approveJoin(j.id))}>
                  Let {j.name} in
                </Btn>
                <Btn small disabled={busy} onClick={() => void run(() => api.declineJoin(j.id))}>
                  No
                </Btn>
              </li>
            ))}
          </ul>
        )}

        {view.admin && (
          <div>
            <Btn disabled={busy} onClick={() => setInviting(true)}>
              Invite someone
            </Btn>
          </div>
        )}

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
        {view.profile && <Note>This is the “{view.profile}” profile, a separate Puppet Master on this computer.</Note>}
        <div>
          <Btn disabled={busy} onClick={() => setLeaving(true)}>
            Leave this team
          </Btn>
        </div>
      </>
    );
  };

  return (
    <SettingBlock
      label="Team (Labs)"
      description="When your agent pushes, offer to tell your teammates. When theirs pushes, show a card to pull it or have your agent merge it in a separate copy. Nothing runs or gets pushed without your click."
    >
      <div className="st-team">
        {view?.readError && (
          <Note role="alert">
            Your team settings file couldn't be read, so Team is off. Setting it up again here replaces it (the old file is kept next to it as team.json.unreadable).
            <span className="st-team-detail"> {view.readError}</span>
          </Note>
        )}
        {body()}
        {view?.error && <Note role="alert">{view.error}</Note>}
        {error && <Note role="alert">{error}</Note>}
      </div>
      {inviting && <InviteSheet api={api} onClose={() => setInviting(false)} />}
      {removing && (
        <ConfirmSheet
          title={`Remove ${removing.name}?`}
          body="They keep what they already received. The team key changes, so they can't read anything new."
          confirmLabel="Remove"
          onCancel={() => setRemoving(null)}
          onConfirm={() => {
            const id = removing.id;
            setRemoving(null);
            void run(() => api.removeMember(id));
          }}
        />
      )}
      {leaving && (
        <ConfirmSheet
          title="Leave this team?"
          body="You stop sending and receiving team updates on this computer. What you already received stays."
          confirmLabel="Leave"
          onCancel={() => setLeaving(false)}
          onConfirm={() => {
            setLeaving(false);
            void run(() => api.leave());
          }}
        />
      )}
    </SettingBlock>
  );
}

function CreateForm({ busy, onCreate }: { busy: boolean; onCreate: (input: TeamCreateInput) => Promise<boolean> }) {
  const [teamName, setTeamName] = useState('');
  const [name, setName] = useState('');
  const [relay, setRelay] = useState('');
  const [useFolder, setUseFolder] = useState(false);
  const [folder, setFolder] = useState('');
  const where = useFolder ? folder.trim() : relay.trim();
  const ready = !!teamName.trim() && !!name.trim() && !!where;
  return (
    <div className="st-team-section">
      <h3 className="st-team-h">Create a team</h3>
      <label className="st-team-row">
        <span>Team name</span>
        <FieldInput value={teamName} placeholder="Acme" maxLength={60} onChange={(e) => setTeamName(e.target.value)} />
      </label>
      <label className="st-team-row">
        <span>Your name</span>
        <FieldInput value={name} placeholder="Maya" maxLength={40} onChange={(e) => setName(e.target.value)} />
      </label>
      {useFolder ? (
        <label className="st-team-row">
          <span>Shared folder</span>
          <FieldInput value={folder} className="font-mono" placeholder="D:\team\wire" onChange={(e) => setFolder(e.target.value)} />
        </label>
      ) : (
        <label className="st-team-row">
          <span>Relay address</span>
          <FieldInput value={relay} className="font-mono" placeholder="https://relay.example.com" onChange={(e) => setRelay(e.target.value)} />
        </label>
      )}
      {!useFolder && <RelayHelp />}
      <div className="st-team-row">
        <span />
        <div className="st-team-inline">
          <Switch checked={useFolder} label="Use a shared folder instead" onChange={setUseFolder} />
          <span>Use a shared folder instead</span>
        </div>
      </div>
      <div className="st-team-row">
        <span />
        <Btn
          primary
          disabled={busy || !ready}
          onClick={() => void onCreate({ teamName: teamName.trim(), name: name.trim(), relay: useFolder ? '' : relay.trim(), folder: useFolder ? folder.trim() : '' })}
        >
          Create team
        </Btn>
      </div>
    </div>
  );
}

function JoinForm({ busy, onJoin }: { busy: boolean; onJoin: (input: TeamJoinInput) => Promise<boolean> }) {
  const [link, setLink] = useState('');
  const [code, setCode] = useState('');
  const [name, setName] = useState('');
  const [github, setGithub] = useState('');
  const ready = !!link.trim() && code.replace('-', '').length === 8 && !!name.trim();
  return (
    <div className="st-team-section">
      <h3 className="st-team-h">Join with an invite</h3>
      <label className="st-team-row">
        <span>Invite link</span>
        <FieldInput value={link} className="font-mono" placeholder="pm://team/join?…" onChange={(e) => setLink(e.target.value)} />
      </label>
      <label className="st-team-row">
        <span>Code</span>
        <FieldInput value={code} className="font-mono" placeholder="ABCD-EFGH" maxLength={9} onChange={(e) => setCode(formatCode(e.target.value))} />
      </label>
      <label className="st-team-row">
        <span>Your name</span>
        <FieldInput value={name} placeholder="Leo" maxLength={40} onChange={(e) => setName(e.target.value)} />
      </label>
      <label className="st-team-row">
        <span>GitHub login</span>
        <FieldInput value={github} placeholder="Optional" maxLength={39} onChange={(e) => setGithub(e.target.value)} />
      </label>
      <div className="st-team-row">
        <span />
        <Btn primary disabled={busy || !ready} onClick={() => void onJoin({ link: link.trim(), code, name: name.trim(), github: github.trim() || null })}>
          Ask to join
        </Btn>
      </div>
    </div>
  );
}
