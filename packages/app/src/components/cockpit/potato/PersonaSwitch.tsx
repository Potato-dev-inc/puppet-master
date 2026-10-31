// "Fish | Potato": the voice of a bot. The name, the threads, the memory and the mood stay; only the voice changes.
import { PERSONA_LABEL, type TalkPersona } from '../../../lib/potato';
import { SegTabs } from '../SegTabs';

const OPTIONS = (Object.keys(PERSONA_LABEL) as TalkPersona[]).map((id) => ({ id, label: PERSONA_LABEL[id] }));

export function PersonaSwitch({ value, onChange }: { value: TalkPersona; onChange: (next: TalkPersona) => void }) {
  return <SegTabs small label="Voice" options={OPTIONS} value={value} onChange={onChange} />;
}
