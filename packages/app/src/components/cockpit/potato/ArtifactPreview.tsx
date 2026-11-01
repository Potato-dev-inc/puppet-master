// The preview pane of the Artifacts page: the name, what it is, Download, Show in folder and Delete, a Version select when the
// name has more than one version, and the content by kind. Text and Markdown are shown as plain text (never as HTML); HTML
// goes in a sandboxed frame (`ArtifactSandbox`); PDFs and other files say they need a download.
import { useState } from 'react';
import { ARTIFACT_COPY, KIND_LABEL, hasContent, sizeLabel, versionLabel, type Artifact, type ArtifactPreview as Preview } from '../../../lib/potato-artifacts';
import { formatWhen } from '../../../lib/potato';
import { ArtifactSandbox } from './ArtifactSandbox';

export interface ArtifactPreviewProps {
  /** The version on show; null: nothing is selected. */
  preview: Preview | null;
  /** The bot's name for each bot id. */
  botName: (botId: string) => string;
  now: number;
  onOpenVersion: (id: string) => void;
  onDownload: (artifact: Artifact) => void;
  onReveal: (artifact: Artifact) => void;
  /** Deletes this version; the page reads the list again. */
  onRemove: (artifact: Artifact) => void;
}

export function ArtifactPreview({ preview, botName, now, onOpenVersion, onDownload, onReveal, onRemove }: ArtifactPreviewProps) {
  const [asking, setAsking] = useState(false);
  if (!preview) return null;
  const a = preview.artifact;
  return (
    <article className="ck-artifact-view" aria-label={a.name}>
      <header className="ck-artifact-view-head">
        <div className="ck-artifact-view-title">
          <h3 className="ck-artifact-name">{a.name}</h3>
          <p className="ck-sub ck-artifact-meta">{`${KIND_LABEL[a.kind]} · ${sizeLabel(a.size)} · ${botName(a.bot)} · ${formatWhen(a.created, now)}`}</p>
        </div>
        <div className="ck-artifact-actions">
          <button type="button" className="ck-btn ck-btn--secondary" onClick={() => onDownload(a)}>
            {ARTIFACT_COPY.download}
          </button>
          <button type="button" className="ck-btn ck-btn--secondary" onClick={() => onReveal(a)}>
            {ARTIFACT_COPY.reveal}
          </button>
          <button type="button" className="ck-artifact-text-btn" onClick={() => setAsking(true)}>
            {ARTIFACT_COPY.remove}
          </button>
        </div>
      </header>

      {asking && (
        <div className="ck-artifact-confirm" role="group" aria-label={`Delete version ${a.version} of ${a.name}`}>
          <p>{`Delete version ${a.version} of ${a.name}? ${ARTIFACT_COPY.removeAsk}`}</p>
          <div className="ck-artifact-actions">
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => setAsking(false)}>
              {ARTIFACT_COPY.cancel}
            </button>
            <button
              type="button"
              className="ck-btn ck-btn--primary"
              onClick={() => {
                setAsking(false);
                onRemove(a);
              }}
            >
              {ARTIFACT_COPY.remove}
            </button>
          </div>
        </div>
      )}

      {preview.versions.length > 1 && (
        <label className="ck-artifact-version">
          <span className="ck-sub">{ARTIFACT_COPY.version}</span>
          <select value={a.id} onChange={(e) => onOpenVersion(e.target.value)}>
            {preview.versions.map((v) => (
              <option key={v.id} value={v.id}>
                {versionLabel(v, formatWhen(v.created, now))}
              </option>
            ))}
          </select>
        </label>
      )}

      <div className="ck-artifact-body">
        {a.kind === 'image' && preview.image && <img className="ck-artifact-image" src={preview.image} alt={a.name} />}
        {a.kind === 'html' && preview.text !== null && <ArtifactSandbox html={preview.text} title={a.name} />}
        {a.kind !== 'html' && a.kind !== 'image' && preview.text !== null && (
          <>
            <pre className="ck-artifact-text">{preview.text}</pre>
            {preview.truncated && <p className="ck-sub ck-artifact-note">{ARTIFACT_COPY.truncated}</p>}
          </>
        )}
        {!hasContent(preview) && <p className="ck-sub ck-artifact-note">{ARTIFACT_COPY.unsupported}</p>}
      </div>
    </article>
  );
}
