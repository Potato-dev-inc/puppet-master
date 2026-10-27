// The companion's page in the cockpit's main area (SPEC section 8): the face, the name, the state line and the
// buttons on top; the companion's own chat in the middle; a 280 px rail on the right with the memory, the pending
// notes, the check-ins and the setup. The chat is the Chat tab's panel for the companion's pane, so its composer
// is the same one.
import { useEffect, useState } from 'react';
import type { CompanionTransport, CompanionView } from '../../../lib/companion';
import { stateWord } from '../../../lib/companion';
import { useCompanionMemory } from '../../../hooks/useCompanions';
import { Avatar } from '../../companion/Avatar';
import { SessionChatPanel } from '../../chat/SessionChatPanel';
import { IconInfo, IconMore } from '../Icons';
import { Sheet } from '../Sheet';
import { CheckInsCard } from './CheckInsCard';
import { MemoryCard } from './MemoryCard';
import { PendingCard } from './PendingCard';
import { isValidName, stateLineOf } from './model';
import '../../../styles/cockpit-companion.css';

export interface CompanionPageProps {
  companion: CompanionView;
  transport: CompanionTransport;
  /** Opens the companion's pane in the normal pane view (its terminal). */
  onShowTerminal: (paneId: string) => void;
  /** Opens the Inbox, where the card that needs the person sits. */
  onOpenCard: () => void;
  /** Says a short sentence about a failed command. */
  onNotify: (text: string) => void;
  /** Called after a command, so the list is read again at once. */
  onChanged: () => void;
  /** Called after the companion is archived: the page leaves. */
  onArchived: () => void;
}

export function CompanionPage(props: CompanionPageProps) {
  const { companion, transport, onShowTerminal, onOpenCard, onNotify, onChanged, onArchived } = props;
  const [railOpen, setRailOpen] = useState(true);
  const [busy, setBusy] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState(companion.name);
  const { memory, setMemory } = useCompanionMemory(transport, companion.id, `${companion.state}|${companion.pendingMemory}`);
  // A companion that slept or napped has a sleep entry in its feed. Its next session starts empty, so the empty chat
  // says why. (A brand-new companion has no sleep entry yet, so it gets no such line.)
  const [sleptBefore, setSleptBefore] = useState(false);
  useEffect(() => {
    let live = true;
    transport
      .feed(companion.id)
      .then((items) => {
        if (live) setSleptBefore(items.some((item) => item.kind === 'slept' || item.kind === 'napped'));
      })
      .catch(() => {
        if (live) setSleptBefore(false);
      });
    return () => {
      live = false;
    };
  }, [transport, companion.id, companion.state]);

  const sleeping = companion.state === 'sleeping';
  const name = companion.name;
  const folder = companion.folder ?? companion.folderName;
  // The desktop view carries the list; without it (an older backend) the card offers no editor.
  const checkins = companion.checkins;

  // One command at a time; a failure becomes a sentence, never a thrown error.
  const run = async (command: () => Promise<unknown>, failed: string) => {
    if (busy) return;
    setBusy(true);
    try {
      await command();
      onChanged();
    } catch (err) {
      onNotify(err instanceof Error ? err.message : failed);
    } finally {
      setBusy(false);
    }
  };

  const toggleSleep = () => run(() => (sleeping ? transport.wake(companion.id) : transport.sleep(companion.id)), 'Could not change the state.');

  const startRename = () => {
    setDraftName(name);
    setRenaming(true);
  };
  const finishRename = () => {
    setRenaming(false);
    const next = draftName.trim();
    if (!isValidName(next) || next === name) return;
    void run(() => transport.update(companion.id, { name: next }), 'Could not rename it.');
  };

  return (
    <div className="ck-companion-page">
      <div className="ck-companion-main">
        <header className="ck-companion-head">
          <Avatar id={companion.avatar} size={72} state={companion.state} name={name} className="ck-companion-face-big" />
          <div className="ck-companion-head-main">
            {renaming ? (
              <input
                className="ck-title-input ck-companion-name-input"
                aria-label="Companion name"
                autoFocus
                maxLength={24}
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                onBlur={finishRename}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') e.currentTarget.blur();
                  if (e.key === 'Escape') {
                    setDraftName(name);
                    setRenaming(false);
                  }
                }}
              />
            ) : (
              <button type="button" className="ck-companion-name" onClick={startRename} title="Rename">
                {name}
              </button>
            )}
            <p className="ck-sub ck-companion-stateline">{stateLineOf(companion, stateWord(companion.state))}</p>
            {companion.needs && (
              <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary ck-companion-open-card" onClick={onOpenCard}>
                Open card
              </button>
            )}
          </div>
          <div className="ck-companion-head-actions">
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void toggleSleep()} disabled={busy}>
              {sleeping ? `Wake up` : 'Sleep'}
            </button>
            {companion.paneId && (
              <button type="button" className="ck-btn ck-btn--plain" onClick={() => onShowTerminal(companion.paneId!)}>
                Show terminal
              </button>
            )}
            <div className="ck-menu-wrap">
              <button type="button" className="ck-icon-btn" aria-label="More" aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}>
                <IconMore />
              </button>
              {menuOpen && (
                <div className="ck-menu ck-menu--wide" role="menu">
                  <button type="button" className="ck-menu-item" role="menuitem" onClick={() => { setMenuOpen(false); void run(() => transport.restart(companion.id), 'Could not restart it.'); }}>
                    Restart
                  </button>
                  <button type="button" className="ck-menu-item" role="menuitem" onClick={() => { setMenuOpen(false); setArchiveOpen(true); }}>
                    Archive
                  </button>
                </div>
              )}
            </div>
            <button
              type="button"
              className="ck-icon-btn"
              onClick={() => setRailOpen((v) => !v)}
              aria-expanded={railOpen}
              aria-label={railOpen ? 'Hide details' : 'Show details'}
              title={railOpen ? 'Hide details' : 'Show details'}
            >
              <IconInfo />
            </button>
          </div>
        </header>

        {sleeping ? (
          <div className="ck-companion-asleep">
            <p className="ck-sub">Sleeping. Wake me when you need me.</p>
            <button type="button" className="ck-btn ck-btn--primary" onClick={() => void toggleSleep()} disabled={busy}>
              Wake up {name}
            </button>
          </div>
        ) : companion.paneId ? (
          <div className="ck-scroll ck-fade-in ck-companion-chat" key={companion.paneId}>
            <SessionChatPanel
              paneId={companion.paneId}
              reply={{ blockedReason: null }}
              emptyNote={
                sleptBefore ? (
                  <div className="ck-companion-divider" role="note">
                    <span className="ck-companion-divider-rule" aria-hidden="true" />
                    <span>New session after a nap or wake-up</span>
                    <span className="ck-companion-divider-rule" aria-hidden="true" />
                  </div>
                ) : undefined
              }
            />
          </div>
        ) : (
          <p className="ck-sub ck-companion-wait">Waking up…</p>
        )}
      </div>

      {railOpen && (
        <aside className="ck-companion-rail" aria-label="Companion details">
          {memory && (
            <MemoryCard
              key={companion.id}
              companionId={companion.id}
              name={name}
              memory={memory}
              transport={transport}
              onChanged={(next) => {
                setMemory(next);
                onChanged();
              }}
            />
          )}
          {memory && (
            <PendingCard
              companionId={companion.id}
              name={name}
              items={memory.pending}
              transport={transport}
              onChanged={(next) => {
                setMemory(next);
                onChanged();
              }}
            />
          )}
          <CheckInsCard companionId={companion.id} checkins={checkins} nextCheckin={companion.nextCheckin} transport={transport} onChanged={() => onChanged()} />
          <section className="ck-companion-card" aria-labelledby="ck-companion-setup-card">
            <h2 className="ck-companion-card-title" id="ck-companion-setup-card">
              Setup
            </h2>
            <dl className="ck-kv">
              <dt>Agent</dt>
              <dd>{companion.agentLabel}</dd>
              <dt>Folder</dt>
              <dd className="ck-mono">{companion.folderName}</dd>
            </dl>
            <div className="ck-companion-meta">
              <button type="button" className="ck-btn ck-btn--sm ck-btn--secondary" onClick={() => void run(() => transport.openFolder(companion.id), 'Could not open the folder.')}>
                Open folder
              </button>
            </div>
            <p className="ck-sub ck-companion-privacy">Its files stay on this computer, in {folder}. It never posts anything in public.</p>
          </section>
        </aside>
      )}

      {archiveOpen && (
        <Sheet onClose={() => setArchiveOpen(false)} labelledBy="ck-companion-archive-title" className="ck-companion-sheet">
          <h3 className="ck-section" id="ck-companion-archive-title">
            Archive {name}?
          </h3>
          <p className="ck-sub">It stops and leaves the list. Its files stay in its folder.</p>
          <div className="ck-sheet-actions">
            <button type="button" className="ck-btn ck-btn--plain" onClick={() => setArchiveOpen(false)}>
              Cancel
            </button>
            <button
              type="button"
              className="ck-btn ck-btn--primary"
              onClick={() => {
                setArchiveOpen(false);
                void run(async () => {
                  await transport.archive(companion.id);
                  onArchived();
                }, 'Could not archive it.');
              }}
            >
              Archive
            </button>
          </div>
        </Sheet>
      )}
    </div>
  );
}
