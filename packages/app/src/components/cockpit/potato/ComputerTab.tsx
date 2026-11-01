// The Computer tab of the bot panel: the live preview of the bot's screen with Take control (ComputerPanel), the upkeep of
// the sandbox under it (ComputerMaintenance), or a short line that says why there is none: Sandbox is off in Labs, or the
// bot talks through the person's agents. While the sandbox is not set up there is a "Set up the computer" card that opens
// the wizard.
import { useState, type ReactNode } from 'react';
import type { AvatarId } from '../../../lib/companion';
import { PANEL_COPY } from '../../../lib/potato-panel';
import type { SandboxTransport } from '../../../lib/potato-sandbox';
import type { ScreenTransport } from '../../../lib/potatoScreen';
import { useSandbox } from '../../settings/sandbox/useSandbox';
import { ComputerMaintenance, SetUpCard, useSandboxActions } from './ComputerMaintenance';
import { ComputerPanel } from './ComputerPanel';
import { SandboxWizardHost } from './SandboxWizardHost';

export interface ComputerTabProps {
  bot: { id: string; name: string };
  avatar?: AvatarId;
  screen?: ScreenTransport;
  /** The sandbox's commands. Absent: the tab has no upkeep section and shows the screen as it is. */
  sandbox?: SandboxTransport;
  through: boolean;
  onNotify: (text: string) => void;
  /** Opens Settings > Potato Bot (where the Sandbox rows are). */
  onOpenSettings?: () => void;
}

export function ComputerTab({ bot, avatar, screen, sandbox, through, onNotify, onOpenSettings }: ComputerTabProps) {
  if (through) return <p className="ck-sub ck-potato-panel-empty">{PANEL_COPY.noComputerThrough}</p>;
  const off = <p className="ck-sub ck-potato-panel-empty">{PANEL_COPY.noComputer(bot.name)}</p>;
  if (!screen) return off;
  if (!sandbox) return <ComputerPanel bot={bot} avatar={avatar} transport={screen} onNotify={onNotify} whenOff={off} />;
  return <WithSandbox bot={bot} avatar={avatar} screen={screen} sandbox={sandbox} onNotify={onNotify} onOpenSettings={onOpenSettings} whenOff={off} />;
}

function WithSandbox({
  bot,
  avatar,
  screen,
  sandbox,
  onNotify,
  onOpenSettings,
  whenOff,
}: Pick<ComputerTabProps, 'bot' | 'avatar' | 'onNotify' | 'onOpenSettings'> & { screen: ScreenTransport; sandbox: SandboxTransport; whenOff: ReactNode }) {
  const { view, refresh } = useSandbox(sandbox);
  const [wizard, setWizard] = useState(false);
  const actions = useSandboxActions(sandbox, refresh, onNotify);
  const notSetUp = view !== null && (view.state === 'not_installed' || view.state === 'installing');
  return (
    <>
      <ComputerPanel
        bot={bot}
        avatar={avatar}
        transport={screen}
        onNotify={onNotify}
        whenOff={whenOff}
        sandboxState={view ? view.state : null}
        onWake={() => void actions.start()}
        waking={actions.busy === 'start'}
        notSetUp={notSetUp && view ? <SetUpCard botName={bot.name} view={view} onSetUp={() => setWizard(true)} /> : undefined}
        after={view && !notSetUp ? <ComputerMaintenance transport={sandbox} view={view} refresh={refresh} onOpenSettings={onOpenSettings} onNotify={onNotify} /> : null}
      />
      {wizard && (
        <SandboxWizardHost
          transport={sandbox}
          onClose={() => {
            setWizard(false);
            void refresh();
          }}
        />
      )}
    </>
  );
}
