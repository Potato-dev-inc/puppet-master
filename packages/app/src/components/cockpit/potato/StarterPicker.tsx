// The "New bot" picker (F-2): six cards from the starter data. Each card says in one sentence what its bot will do.
// "Make this bot" calls the existing create, add, save and routine calls (lib/potato-starters). A card that needs the
// computer says so and offers the computer's set-up button. "Set up by hand" keeps the old companion sheet.
import { useState } from 'react';
import { plainError } from '../../../lib/potato';
import { COMPUTER_COPY } from '../../../lib/potato-computer';
import { STARTERS, STARTER_COPY, makeStarterBot, type StarterTemplate, type StarterTransports } from '../../../lib/potato-starters';
import '../../../styles/cockpit-potato-starters.css';

export interface StarterPickerProps {
  transports: StarterTransports;
  /** Called with the new bot's id once every step has run. */
  onMade: (botId: string) => void;
  /** Opens the computer's set-up wizard. Absent: the set-up button is not drawn. */
  onSetUpComputer?: () => void;
  /** Opens the old setup sheet (a custom agent, persona and folder). */
  onSetUpByHand: () => void;
  onClose: () => void;
}

export function StarterPicker({ transports, onMade, onSetUpComputer, onSetUpByHand, onClose }: StarterPickerProps) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const make = async (starter: StarterTemplate) => {
    if (busy) return;
    setBusy(starter.id);
    setError(null);
    try {
      const botId = await makeStarterBot(starter, transports);
      onMade(botId);
    } catch (err) {
      setError(plainError(err, 'Could not make the bot.'));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="ck-potato-starters-overlay" role="presentation">
      <section className="ck-potato-starters" role="dialog" aria-modal="true" aria-label={STARTER_COPY.title}>
        <header className="ck-potato-starters-head">
          <h2 className="ck-potato-starters-title">{STARTER_COPY.title}</h2>
          <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={onClose}>
            Close
          </button>
        </header>
        <p className="ck-sub ck-potato-starters-intro">{STARTER_COPY.intro}</p>
        {error && (
          <p role="alert" className="ck-potato-starters-error">
            {error}
          </p>
        )}
        <div className="ck-potato-starters-grid">
          {STARTERS.map((starter) => (
            <article key={starter.id} className="ck-potato-starter" data-starter={starter.id}>
              <h3 className="ck-potato-starter-name">{starter.name}</h3>
              <p className="ck-potato-starter-doing">{starter.doing}</p>
              {starter.needsComputer && (
                <p className="ck-potato-starter-note">
                  {STARTER_COPY.needsComputer}
                  {onSetUpComputer && (
                    <button type="button" className="ck-btn ck-btn--plain ck-btn--sm ck-potato-starter-setup" onClick={onSetUpComputer}>
                      {COMPUTER_COPY.setUpButton}
                    </button>
                  )}
                </p>
              )}
              <button
                type="button"
                className="ck-btn ck-btn--secondary ck-potato-starter-make"
                aria-label={`Make ${starter.name}`}
                disabled={busy !== null}
                onClick={() => void make(starter)}
              >
                {busy === starter.id ? 'Making…' : 'Make this bot'}
              </button>
            </article>
          ))}
        </div>
        <footer className="ck-potato-starters-foot">
          <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={onSetUpByHand}>
            {STARTER_COPY.setByHand}
          </button>
        </footer>
      </section>
    </div>
  );
}
