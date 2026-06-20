import logoUrl from '../assets/branding/logo.svg';
import type { BootStep } from '../hooks/useBootGate';

interface Props {
  steps: BootStep[];
  progress: number;
  exiting?: boolean;
}

function StepIcon({ status }: { status: BootStep['status'] }) {
  if (status === 'done') {
    return <span className="pm-boot-step-icon pm-boot-step-icon--done" aria-hidden>✓</span>;
  }
  if (status === 'active') {
    return <span className="pm-boot-step-icon pm-boot-step-icon--active" aria-hidden />;
  }
  return <span className="pm-boot-step-icon pm-boot-step-icon--pending" aria-hidden />;
}

export function LoadingScreen({ steps, progress, exiting = false }: Props) {
  const percent = Math.round(progress * 100);

  return (
    <main
      className={['pm-boot-screen', exiting ? 'pm-boot-screen--exit' : ''].filter(Boolean).join(' ')}
      aria-busy={!exiting}
      aria-label="Puppet Master is starting"
    >
      <div className="pm-boot-grid-bg" aria-hidden />

      <div className="pm-boot-layout">
        <header className="pm-boot-header">
          <img src={logoUrl} alt="" className="pm-boot-logo" draggable={false} />
          <div>
            <p className="pm-boot-kicker">Puppet Master</p>
            <h1 className="pm-boot-title">Starting control room</h1>
          </div>
        </header>

        <div className="pm-boot-stage">
          <div className="pm-boot-pane-grid" aria-hidden>
            <div className="pm-boot-pane pm-boot-pane--a">
              <span className="pm-boot-pane-label">planner</span>
              <div className="pm-boot-pane-lines">
                <span />
                <span />
                <span className="pm-boot-pane-lines--short" />
              </div>
            </div>
            <div className="pm-boot-pane pm-boot-pane--b">
              <span className="pm-boot-pane-label">bridge</span>
              <div className="pm-boot-pane-lines">
                <span />
                <span className="pm-boot-pane-lines--short" />
              </div>
            </div>
            <div className="pm-boot-pane pm-boot-pane--c">
              <span className="pm-boot-pane-label">tests</span>
              <div className="pm-boot-pane-lines">
                <span />
                <span />
                <span />
              </div>
            </div>
            <div className="pm-boot-scan" />
            <div className="pm-boot-node" />
          </div>

          <div className="pm-boot-log">
            <ul className="pm-boot-steps">
              {steps.map((step) => (
                <li
                  key={step.id}
                  className={[
                    'pm-boot-step',
                    step.status === 'active' ? 'pm-boot-step--active' : '',
                    step.status === 'done' ? 'pm-boot-step--done' : '',
                  ]
                    .filter(Boolean)
                    .join(' ')}
                >
                  <StepIcon status={step.status} />
                  <span className="pm-boot-step-text">{step.label}</span>
                </li>
              ))}
            </ul>
          </div>
        </div>

        <footer className="pm-boot-footer">
          <div className="pm-boot-progress-track">
            <div className="pm-boot-progress-fill" style={{ width: `${percent}%` }} />
            <div className="pm-boot-progress-glow" style={{ left: `${percent}%` }} />
          </div>
          <div className="pm-boot-footer-meta">
            <span>Boot sequence {percent}%</span>
            <span className="pm-boot-cursor" aria-hidden>
              _
            </span>
          </div>
        </footer>
      </div>
    </main>
  );
}
