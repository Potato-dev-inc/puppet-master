// Opens the sandbox set up wizard from the Computer tab. The wizard was made for the settings window and draws inside a
// `.settings` layer (its tokens and sheet styles hang on it), so this gives it one: an empty, see-through layer over the
// whole window that holds nothing but the wizard's own scrim.
import { createPortal } from 'react-dom';
import type { SandboxTransport } from '../../../lib/potato-sandbox';
import { SandboxWizard } from '../../settings/sandbox/SandboxWizard';
import '../../../styles/settings.css';
import '../../../styles/potato-sandbox.css';
import '../../../styles/cockpit-potato-overlay.css';

export function SandboxWizardHost({ transport, onClose }: { transport: SandboxTransport; onClose: () => void }) {
  return createPortal(
    <div className="settings ck-potato-wizard-host">
      <SandboxWizard transport={transport} onClose={onClose} />
    </div>,
    document.querySelector('.cockpit') ?? document.body,
  );
}
