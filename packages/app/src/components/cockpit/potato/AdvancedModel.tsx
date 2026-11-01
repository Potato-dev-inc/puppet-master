// "Model" on the Advanced tab: which talk brain this bot uses. "Use the default" follows the Settings choice. The
// engine starts the bot on the new brain from its next message.
import type { BrainKind } from '../../../lib/potato';
import { ADVANCED_COPY, brainChoices, brainFromValue, brainValue } from '../../../lib/potato-botsettings';
import { AdvancedSection } from './AdvancedSection';

export interface AdvancedModelProps {
  brain: BrainKind | null;
  defaultBrain: BrainKind;
  onChange: (brain: BrainKind | null) => void;
}

export function AdvancedModel({ brain, defaultBrain, onChange }: AdvancedModelProps) {
  return (
    <AdvancedSection title={ADVANCED_COPY.model} line={ADVANCED_COPY.modelLine} id="ck-adv-model">
      <select
        className="ck-potato-adv-input"
        aria-labelledby="ck-adv-model"
        value={brainValue(brain)}
        onChange={(event) => onChange(brainFromValue(event.target.value))}
      >
        {brainChoices(defaultBrain).map((choice) => (
          <option key={choice.value} value={choice.value}>
            {choice.label}
          </option>
        ))}
      </select>
    </AdvancedSection>
  );
}
