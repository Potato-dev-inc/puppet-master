// Phone / PWA: shown under the terminal when the PC refused what was typed (a 403 from the
// managed-pane guard, or a socket `input-error`). A refused write is never silent.
export function InputRefusedNote({ text }: { text: string }) {
  return (
    <div className="ph-note ph-note--refused" role="status" data-testid="input-refused-note">
      <span>{text}</span>
    </div>
  );
}
