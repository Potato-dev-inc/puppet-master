// Phone: shows the address of the dev server an agent started, with a note. The tunnel never
// proxies dev-server ports (that would expose whatever the agent is running to the internet), so
// the page can only be opened on the computer.
import type { PreviewUrl } from '../../lib/pane-preview';

export function PreviewNote({ preview }: { preview: PreviewUrl }) {
  return (
    <div className="ph-note" role="note">
      <b>Preview</b>
      <code>{preview.url}</code>
      <span>Open on your PC</span>
    </div>
  );
}
