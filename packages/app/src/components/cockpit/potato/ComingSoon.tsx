// A tab that is not built yet says so, in one short line.
import { PANEL_COPY } from '../../../lib/potato-panel';

export function ComingSoon({ line }: { line: string }) {
  return (
    <div className="ck-potato-soon">
      <p className="ck-potato-soon-title">{PANEL_COPY.comingSoon}</p>
      <p className="ck-sub">{line}</p>
    </div>
  );
}
