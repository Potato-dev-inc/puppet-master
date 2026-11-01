// An HTML artifact, drawn in an iframe with `sandbox=""`: no scripts, no forms, no same-origin access, so it cannot reach the
// app or the person's account. The document also carries a policy that blocks every network load. The page adds the
// "Preview only" pill, as the reference app does.
import { ARTIFACT_COPY, artifactFrameDoc } from '../../../lib/potato-artifacts';

export interface ArtifactSandboxProps {
  html: string;
  /** The frame's accessible name: the artifact's name. */
  title: string;
}

export function ArtifactSandbox({ html, title }: ArtifactSandboxProps) {
  return (
    <div className="ck-artifact-frame-wrap">
      <iframe className="ck-artifact-frame" title={title} sandbox="" srcDoc={artifactFrameDoc(html)} />
      <span className="ck-artifact-pill">{ARTIFACT_COPY.isolated}</span>
    </div>
  );
}
