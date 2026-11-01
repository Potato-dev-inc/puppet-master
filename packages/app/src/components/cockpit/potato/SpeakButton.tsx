// The "Speak" button in a bot reply's hover toolbar (after Copy and Reply): reads it aloud with the system voice, or stops it while it is being read.
// Not shown when this webview has no speech, or when the reply has nothing to say. Only one reply is read at a time
// (lib/potato-speech keeps the queue); the button shows Stop on the one that is being read.
import { useSyncExternalStore } from 'react';
import type { PotatoVoice } from '../../../lib/potato';
import { normalizeVoice, speakable, speakingMessageId, speakReply, speechSupported, stopSpeaking, subscribeSpeaking } from '../../../lib/potato-speech';
import '../../../styles/cockpit-potato-voice.css';

export interface SpeakButtonProps {
  /** The message's id: the button shows Stop while this message is the one being read. */
  id: string;
  text: string;
  voice?: PotatoVoice;
}

export function SpeakButton({ id, text, voice }: SpeakButtonProps) {
  const speakingId = useSyncExternalStore(subscribeSpeaking, speakingMessageId, speakingMessageId);
  if (!speechSupported() || !speakable(text)) return null;
  const on = speakingId === id;
  return (
    <button
      type="button"
      className={`ck-potato-action ck-potato-speak${on ? ' is-on' : ''}`}
      aria-pressed={on}
      aria-label={on ? 'Stop reading this reply' : 'Read this reply aloud'}
      title={on ? 'Stop reading this reply' : 'Read this reply aloud'}
      onClick={() => {
        if (on) stopSpeaking();
        else speakReply(id, text, normalizeVoice(voice), true);
      }}
    >
      {on ? 'Stop' : 'Speak'}
    </button>
  );
}
