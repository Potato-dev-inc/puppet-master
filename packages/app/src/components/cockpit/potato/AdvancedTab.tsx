// The Advanced tab of the bot panel (per-bot settings, spec Rakazo 1.9): Name, Model, Instructions, Tools, then the bot's
// own actions (Export chat, Delete). Every section is a labelled card. The tab stays mounted while the panel is open, so
// a draft survives a tab switch. Its state is read once from the engine and replaced by each save's answer.
import { useCallback, useEffect, useState } from 'react';
import type { BrainKind } from '../../../lib/potato';
import { plainError } from '../../../lib/potato';
import { ADVANCED_COPY, tauriBotSettingsTransport, type BotOptions, type BotSettingsTransport, type ToolSwitches } from '../../../lib/potato-botsettings';
import { AdvancedBot } from './AdvancedBot';
import { AdvancedInstructions } from './AdvancedInstructions';
import { AdvancedModel } from './AdvancedModel';
import { AdvancedName } from './AdvancedName';
import { AdvancedTools } from './AdvancedTools';
import { MemoryList } from './MemoryList';
import { NotifySection } from './NotifySection';
import { OpenWork } from './OpenWork';
import '../../../styles/cockpit-potato-advanced.css';
import '../../../styles/cockpit-potato-settings.css';

export interface AdvancedTabProps {
  bot: { id: string; name: string };
  /** The brain a bot gets when it has none of its own (the Settings choice). */
  defaultBrain?: BrainKind;
  /** The engine's calls. The app passes nothing; tests pass a fake. */
  transport?: BotSettingsTransport;
  /** After a change that the page should read again (a rename, a delete). */
  onChanged?: () => void;
  onNotify?: (text: string) => void;
}

export function AdvancedTab({ bot, defaultBrain = 'deepseek', transport = tauriBotSettingsTransport, onChanged, onNotify }: AdvancedTabProps) {
  const [options, setOptions] = useState<BotOptions | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    transport
      .options(bot.id)
      .then(setOptions)
      .catch((e: unknown) => setError(plainError(e, 'Could not read the bot settings.')));
  }, [bot.id, transport]);

  useEffect(() => {
    load();
  }, [load]);

  /** Runs one save. The answer replaces the state; a failure is shown in one line at the top. */
  const apply = useCallback(
    async (call: () => Promise<BotOptions>) => {
      try {
        setOptions(await call());
        setError(null);
        return true;
      } catch (e) {
        setError(plainError(e, 'Could not save that.'));
        return false;
      }
    },
    [],
  );

  const rename = async (name: string): Promise<boolean> => {
    try {
      await transport.rename(bot.id, name);
      setError(null);
      onChanged?.();
      onNotify?.(ADVANCED_COPY.renamed);
      return true;
    } catch (e) {
      setError(plainError(e, 'Could not save the name.'));
      return false;
    }
  };

  if (!options) {
    return <p className="ck-sub ck-potato-adv-loading">{error ?? 'Loading…'}</p>;
  }

  return (
    <div className="ck-potato-adv">
      {error && (
        <p className="ck-potato-adv-error" role="alert">
          {error}
        </p>
      )}
      <AdvancedName name={bot.name} onSave={rename} />
      <AdvancedModel brain={options.brain} defaultBrain={defaultBrain} onChange={(brain) => void apply(() => transport.setBrain(bot.id, brain))} />
      <AdvancedInstructions
        instructions={options.instructions}
        versions={options.versions}
        onSave={(text) => apply(() => transport.saveInstructions(bot.id, text))}
        onRestore={(at) => apply(() => transport.restoreInstructions(bot.id, at))}
      />
      <AdvancedTools
        tools={options.tools}
        sandboxOn={options.sandboxOn}
        onChange={(tools: ToolSwitches) => void apply(() => transport.setTools(bot.id, tools))}
      />
      <NotifySection botId={bot.id} />
      <OpenWork botId={bot.id} />
      <MemoryList botId={bot.id} />
      <AdvancedBot
        bot={bot}
        transport={transport}
        onChanged={onChanged}
        onNotify={onNotify}
      />
    </div>
  );
}
