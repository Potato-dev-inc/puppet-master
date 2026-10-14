// Phone: shown in place of the input box when this phone was paired without "type".
export function TypingNote({ text }: { text: string }) {
  return (
    <div className="ph-note ph-note--typing" role="note" data-testid="typing-note">
      <span>{text}</span>
    </div>
  );
}
