// Preview: the page the agent made, shown from its copy. v0 templates make one html file, so the
// file is shown as it is, in a sandboxed frame (scripts run; nothing can reach the app).
import { useEffect, useState } from 'react';
import { DotSpinner } from '../cockpit/DotSpinner';
import { F, T } from './copy';
import { simpleApi, type PreviewFile, type SimpleApi } from './simple-api';

export function PreviewTab({
  paneId,
  status,
  agent,
  api = simpleApi,
  onNotify,
}: {
  paneId: string;
  status: string;
  agent: string;
  api?: Pick<SimpleApi, 'preview' | 'openPreview'>;
  onNotify: (text: string) => void;
}) {
  const [file, setFile] = useState<PreviewFile | null | undefined>(undefined);
  const working = status === 'running';

  useEffect(() => {
    let cancelled = false;
    const load = () =>
      api
        .preview(paneId)
        .then((f) => !cancelled && setFile(f))
        .catch(() => !cancelled && setFile(null));
    void load();
    // While the agent works the page can appear at any moment.
    const timer = working ? window.setInterval(() => void load(), 3000) : undefined;
    return () => {
      cancelled = true;
      if (timer) window.clearInterval(timer);
    };
  }, [api, paneId, status, working]);

  if (file === undefined || file === null) {
    return (
      <div className="sx-empty" data-testid="preview-empty">
        <span aria-hidden="true">
          <DotSpinner status={working ? 'running' : 'idle'} size={5} />
        </span>
        <p className="sx-lead">{working ? F.agentMaking(agent) : T.previewNone}</p>
        {working && <p className="sx-quiet">{T.previewEmpty}</p>}
      </div>
    );
  }
  return (
    <div className="sx-preview">
      <div className="sx-preview-bar">
        <button
          type="button"
          className="ck-btn ck-btn--secondary ck-btn--sm"
          onClick={() => void api.openPreview(paneId).catch(() => onNotify(T.previewOpenFailed))}
        >
          {T.openIt}
        </button>
      </div>
      {file.html === null ? (
        <div className="sx-empty">
          <p className="sx-lead">{T.previewBig}</p>
        </div>
      ) : (
        <iframe className="sx-frame" title={T.tabPreview} sandbox="allow-scripts allow-forms allow-modals" srcDoc={file.html} />
      )}
    </div>
  );
}
