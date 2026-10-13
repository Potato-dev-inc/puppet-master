// Phone: shows the address of the dev server an agent started, with a note. The tunnel never
// proxies dev-server ports (that would expose whatever the agent is running to the internet), so
// the page can only be opened on the computer.
import type { PreviewUrl } from '../../lib/pane-preview';

export function PreviewNote({ preview }: { preview: PreviewUrl }) {
  return (
    <div className="pwa-preview-note" role="note" style={{ padding: '6px 16px', fontSize: 13, color: 'var(--label-2, #a1a1aa)', borderBottom: '1px solid var(--sep, rgba(255,255,255,.08))' }}>
      <span style={{ fontWeight: 600 }}>Preview </span>
      <code style={{ fontFamily: 'ui-monospace, monospace' }}>{preview.url}</code>
      <span> · Open on your PC</span>
    </div>
  );
}
