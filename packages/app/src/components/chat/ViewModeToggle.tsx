// The Summary | Normal | Verbose switch in the chat header. Small on the desktop; on the phone the buttons get a
// 44px touch area (styles/phone-chat.css).
import { CHAT_VIEW_LABEL, CHAT_VIEW_MODES, type ChatViewMode } from './chatViewMode';

export function ViewModeToggle({ mode, onChange }: { mode: ChatViewMode; onChange: (mode: ChatViewMode) => void }) {
  return (
    <div role="group" aria-label="Chat view" className="ml-auto flex shrink-0 items-center gap-0.5 rounded-md bg-pm-raised p-0.5 ck-chat-modes">
      {CHAT_VIEW_MODES.map((value) => (
        <button
          key={value}
          type="button"
          aria-pressed={mode === value}
          onClick={() => onChange(value)}
          className="rounded px-1.5 py-0.5 text-[11px] text-pm-muted ck-chat-mode"
        >
          {CHAT_VIEW_LABEL[value]}
        </button>
      ))}
    </div>
  );
}
