// A new bot's scripted first message: the bot's own outlined bubble, in its persona, saying what it can do, then the
// choice card. Nothing is stored and nothing calls the brain; ThreadView decides when it shows (a bot with no messages).
import type { TalkPersona } from '../../../lib/potato';
import { firstRunScript, visibleChoices } from '../../../lib/potato-firstrun';
import { ChoiceCard } from './ChoiceCard';
import '../../../styles/cockpit-potato-firstrun.css';

export interface FirstMessageProps {
  name: string;
  persona: TalkPersona;
  /** The sandbox Labs switch is on: "Use the computer" and its line are shown. */
  computerOn: boolean;
  /** Sends the text of a choice as the person's first message. */
  onPick: (text: string) => Promise<void>;
}

export function FirstMessage({ name, persona, computerOn, onPick }: FirstMessageProps) {
  const script = firstRunScript(persona, name, computerOn);
  return (
    <section className="ck-potato-firstrun" aria-label={`Start with ${name}`}>
      <div className="ck-potato-line ck-potato-line--bot is-first">
        <div className="ck-potato-bubble ck-potato-bubble--bot">
          <p className="ck-potato-firstrun-text">{script.greeting}</p>
          <ul className="ck-potato-firstrun-points">
            {script.points.map((point) => (
              <li key={point}>{point}</li>
            ))}
          </ul>
          <p className="ck-potato-firstrun-text">{script.question}</p>
        </div>
      </div>
      <ChoiceCard choices={visibleChoices(computerOn)} onPick={onPick} />
    </section>
  );
}
