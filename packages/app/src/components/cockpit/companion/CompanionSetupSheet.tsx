// "Make your companion" (SPEC section 1, the five steps): a face, a name, an agent, how it talks, and where it lives.
// The agent step reads `companion_agents`; an agent that cannot take messages is shown with its reason and cannot be
// picked. One scroll, one button: "Wake up {name}".
import { useEffect, useState } from 'react';
import type { AvatarId, CompanionAgentOption, CompanionPersona, CompanionTransport, CompanionView } from '../../../lib/companion';
import { shipApi, type ShipApi } from '../../../lib/ship-api';
import { tauri } from '../../../lib/tauri';
import { isValidProjectPath } from '../../../lib/project-path';
import { Avatar } from '../../companion/Avatar';
import { AVATARS } from '../../companion/avatars';
import { Sheet } from '../Sheet';
import { PERSONA_OPTIONS, isValidName } from './model';
import '../../../styles/cockpit-companion.css';

const FACES = Object.keys(AVATARS) as AvatarId[];
const DEFAULT_PERSONA: Exclude<CompanionPersona, 'custom'> = 'cheerful';
// The fresh start (SPEC section 4) happens at 04:00 unless the person changes it later.
const DEFAULT_NAP = { h: 4, m: 0 };

export function CompanionSetupSheet({
  transport,
  ship = shipApi,
  onClose,
  onCreated,
}: {
  transport: CompanionTransport;
  ship?: Pick<ShipApi, 'pickFolder'>;
  onClose: () => void;
  onCreated: (view: CompanionView) => void;
}) {
  const [agents, setAgents] = useState<CompanionAgentOption[] | null>(null);
  const [agentType, setAgentType] = useState<string | null>(null);
  const [avatar, setAvatar] = useState<AvatarId>('mochi');
  const [name, setName] = useState(AVATARS.mochi.name);
  // Once the person types a name, changing the face keeps their name.
  const [nameTouched, setNameTouched] = useState(false);
  const [persona, setPersona] = useState<Exclude<CompanionPersona, 'custom'>>(DEFAULT_PERSONA);
  const [folder, setFolder] = useState<string | null>(null);
  const [folderName, setFolderName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    transport
      .agents()
      .then((list) => {
        if (!live) return;
        setAgents(list);
        setAgentType((cur) => cur ?? list.find((a) => a.ok)?.id ?? null);
      })
      .catch((err: unknown) => {
        if (live) setError(err instanceof Error ? err.message : 'Could not read the agents.');
      });
    // The open project is the starting folder; the person can change it in step 5.
    tauri
      .getProjectPath()
      .then((path) => {
        if (!live || !isValidProjectPath(path)) return;
        setFolder(path);
        setFolderName(path.split(/[\\/]/).filter(Boolean).pop() ?? '');
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [transport]);

  const chosen = agents?.find((a) => a.id === agentType) ?? null;
  const valid = isValidName(name) && !!folder && !!chosen?.ok;

  const pickFace = (id: AvatarId) => {
    setAvatar(id);
    if (!nameTouched) setName(AVATARS[id].name);
  };

  const changeFolder = async () => {
    setError(null);
    try {
      const picked = await ship.pickFolder();
      if (picked) {
        setFolder(picked.path);
        setFolderName(picked.name);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not open the folder picker.');
    }
  };

  const create = async () => {
    if (!valid || !folder || !agentType) return;
    setBusy(true);
    setError(null);
    try {
      const view = await transport.create({
        name: name.trim(),
        avatar,
        persona,
        agentType,
        folder,
        autoStart: true,
        memoryReview: true,
        napAt: DEFAULT_NAP,
        checkins: [],
      });
      onCreated(view);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not make the companion.');
    } finally {
      setBusy(false);
    }
  };

  const who = name.trim() || 'it';
  return (
    <Sheet onClose={busy ? undefined : onClose} labelledBy="ck-companion-setup-title" className="ck-companion-sheet ck-companion-setup">
      <h2 className="ck-section" id="ck-companion-setup-title">
        Make your companion
      </h2>
      <p className="ck-sub">Your own helper that is always on. It runs on your agent and your plan.</p>

      <fieldset className="ck-companion-step">
        <legend className="ck-caption">1. Pick a face</legend>
        <div className="ck-companion-faces" role="radiogroup" aria-label="Pick a face">
          {FACES.map((id) => (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={avatar === id}
              className={`ck-companion-face-pick${avatar === id ? ' is-picked' : ''}`}
              onClick={() => pickFace(id)}
              title={AVATARS[id].name}
            >
              <Avatar id={id} size={36} state="idle" name={AVATARS[id].name} />
            </button>
          ))}
        </div>
      </fieldset>

      <label className="ck-companion-step ck-companion-field">
        <span className="ck-caption">2. Give it a name</span>
        <input
          className="ck-companion-input"
          value={name}
          maxLength={24}
          onChange={(e) => {
            setNameTouched(true);
            setName(e.target.value);
          }}
        />
      </label>

      <fieldset className="ck-companion-step">
        <legend className="ck-caption">3. Which agent should it use?</legend>
        <p className="ck-sub ck-companion-hint">It uses your own plan. No extra cost.</p>
        {agents === null && !error && <p className="ck-sub">Looking for agents…</p>}
        {agents?.length === 0 && <p className="ck-sub">No agents yet. Set up an agent first.</p>}
        <div className="ck-companion-agents" role="radiogroup" aria-label="Which agent should it use?">
          {agents?.map((a) => (
            <button
              key={a.id}
              type="button"
              role="radio"
              aria-checked={agentType === a.id}
              disabled={!a.ok}
              className={`ck-companion-agent${agentType === a.id ? ' is-picked' : ''}`}
              onClick={() => setAgentType(a.id)}
            >
              <span>{a.label}</span>
              {!a.ok && a.reason && <span className="ck-sub ck-companion-reason">{a.reason}</span>}
            </button>
          ))}
        </div>
      </fieldset>

      <fieldset className="ck-companion-step">
        <legend className="ck-caption">4. How should it talk?</legend>
        <div className="ck-companion-chips">
          {PERSONA_OPTIONS.map((o) => (
            <button
              key={o.id}
              type="button"
              aria-pressed={persona === o.id}
              className={`ck-companion-chip${persona === o.id ? ' is-picked' : ''}`}
              onClick={() => setPersona(o.id)}
            >
              {o.label}
            </button>
          ))}
        </div>
      </fieldset>

      <fieldset className="ck-companion-step">
        <legend className="ck-caption">5. Where does it live?</legend>
        <div className="ck-companion-folder">
          <span className="ck-companion-folder-name" title={folder ?? undefined}>
            {folderName || 'No folder yet'}
          </span>
          <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={() => void changeFolder()} disabled={busy}>
            Change folder
          </button>
        </div>
      </fieldset>

      {error && (
        <p className="ck-memory-msg ck-memory-err" role="alert">
          {error}
        </p>
      )}
      <p className="ck-sub ck-companion-privacy">Its files stay on this computer, in {folder ?? 'the folder you pick'}. It never posts anything in public.</p>

      <div className="ck-sheet-actions">
        <button type="button" className="ck-btn ck-btn--plain" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button type="button" className="ck-btn ck-btn--primary" onClick={() => void create()} disabled={!valid || busy}>
          {busy ? 'Waking up…' : `Wake up ${who}`}
        </button>
      </div>
    </Sheet>
  );
}
