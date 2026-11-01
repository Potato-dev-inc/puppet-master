// Settings > Potato Bot > Models and fallbacks (a sub-page). The talk brain choice (moved here from the Potato Bot page,
// the same control) and the ordered fallback list: when the talk brain is out of balance or not answering, the next
// brain in the list is tried for that message. Keys are not here: each brain keeps its own (the keychain, or its sign-in).
import { useState } from 'react';
import { brainLabel, plainError, type BrainKind, type PotatoSettings, type PotatoTransport } from '../../../lib/potato';
import { Btn, FieldSelect, Note, SettingBlock, SettingRow } from '../settings-ui';
import { usePotatoSettings } from './usePotatoSettings';

/** What the talk brain does and costs, under its name. */
export const BRAIN_NOTE: Record<BrainKind, string> = {
  deepseek: 'Pays DeepSeek per message. Usually cents a day.',
  go: 'Uses your OpenCode Go sign-in.',
  agents: 'No extra cost.',
};
const BRAINS: BrainKind[] = ['deepseek', 'go', 'agents'];
/** The brains that can be a fallback: the two that run a model. "Through my agents" runs no brain of its own. */
const FALLBACK_BRAINS: BrainKind[] = ['deepseek', 'go'];

/** The list with the brain at `from` moved to `to` (the other brains keep their order). Out of range: the same list. */
export function moveBrain(list: BrainKind[], from: number, to: number): BrainKind[] {
  if (from < 0 || to < 0 || from >= list.length || to >= list.length || from === to) return [...list];
  const next = [...list];
  const [kind] = next.splice(from, 1);
  next.splice(to, 0, kind);
  return next;
}

type Message = { ok: boolean; text: string } | null;

export function ModelsPage({ transport }: { transport: PotatoTransport }) {
  const { settings, failed, save } = usePotatoSettings(transport);
  const [message, setMessage] = useState<Message>(null);
  const [pick, setPick] = useState<BrainKind | ''>('');

  if (!settings) {
    return failed ? <Note role="alert">{failed}</Note> : <span className="pot-sub-loading" aria-busy="true">Loading…</span>;
  }

  const brain = settings.brain;
  const fallbacks = settings.fallbacks ?? [];
  const off = brain === 'agents';
  const addable = FALLBACK_BRAINS.filter((kind) => kind !== brain && !fallbacks.includes(kind));

  const write = async (change: Partial<PotatoSettings>, done: string) => {
    setMessage(null);
    try {
      await save(change);
      setMessage({ ok: true, text: done });
    } catch (error) {
      setMessage({ ok: false, text: plainError(error, 'Could not save that.') });
    }
  };

  const move = (index: number, to: number) => void write({ fallbacks: moveBrain(fallbacks, index, to) }, 'Order saved.');

  return (
    <div className="pot-models">
      <SettingRow label="Talk brain" description={BRAIN_NOTE[brain]}>
        <FieldSelect
          aria-label="Talk brain"
          value={brain}
          onChange={(e) => void write({ brain: e.target.value as BrainKind }, 'Saved.')}
        >
          {BRAINS.map((kind) => (
            <option key={kind} value={kind}>
              {brainLabel(kind)}
            </option>
          ))}
        </FieldSelect>
      </SettingRow>

      <SettingBlock
        label="Fallbacks"
        description="If the talk brain is busy, out of balance or not answering, the next brain in this list is tried for that message, in order. Each new run starts on the talk brain again. Each brain keeps its own key or sign-in."
      >
        {off && <Note>Fallbacks do not apply to Through my agents.</Note>}
        {fallbacks.length === 0 ? (
          <span className="pot-sub-empty">None yet. If the talk brain fails, you will see the error.</span>
        ) : (
          <ol className="pot-fb-list" aria-label="Fallback brains, in order">
            {fallbacks.map((kind, index) => (
              <li key={kind} className="pot-fb-item">
                <span className="pot-fb-name">
                  <span className="pot-fb-index">{index + 1}.</span> {brainLabel(kind)}
                </span>
                <span className="pot-fb-actions">
                  <Btn small disabled={off || index === 0} aria-label={`Move ${brainLabel(kind)} up`} onClick={() => move(index, index - 1)}>
                    Up
                  </Btn>
                  <Btn small disabled={off || index === fallbacks.length - 1} aria-label={`Move ${brainLabel(kind)} down`} onClick={() => move(index, index + 1)}>
                    Down
                  </Btn>
                  <Btn
                    small
                    disabled={off}
                    aria-label={`Remove ${brainLabel(kind)}`}
                    onClick={() => void write({ fallbacks: fallbacks.filter((k) => k !== kind) }, 'Removed.')}
                  >
                    Remove
                  </Btn>
                </span>
              </li>
            ))}
          </ol>
        )}
        {addable.length > 0 && !off && (
          <div className="pot-fb-add">
            <FieldSelect aria-label="Add a fallback" value={pick} onChange={(e) => setPick(e.target.value as BrainKind | '')}>
              <option value="">Choose a brain</option>
              {addable.map((kind) => (
                <option key={kind} value={kind}>
                  {brainLabel(kind)}
                </option>
              ))}
            </FieldSelect>
            <Btn
              disabled={pick === ''}
              onClick={() => {
                if (pick === '') return;
                setPick('');
                void write({ fallbacks: [...fallbacks, pick] }, 'Added.');
              }}
            >
              Add
            </Btn>
          </div>
        )}
        {!off && addable.length === 0 && fallbacks.length > 0 && <span className="pot-sub-empty">Every other brain is in the list.</span>}
        {message && <Note role={message.ok ? 'status' : 'alert'}>{message.text}</Note>}
      </SettingBlock>
    </div>
  );
}
