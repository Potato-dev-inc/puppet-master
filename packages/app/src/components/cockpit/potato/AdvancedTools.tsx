// "Tools" on the Advanced tab: three switches. A switch that is off makes the engine refuse that tool at once. The
// computer row is shown only when the sandbox Labs switch is on.
import { ADVANCED_COPY, toolRows, type ToolName, type ToolSwitches } from '../../../lib/potato-botsettings';
import { AdvancedSection } from './AdvancedSection';

export interface AdvancedToolsProps {
  tools: ToolSwitches;
  sandboxOn: boolean;
  onChange: (tools: ToolSwitches) => void;
}

export function AdvancedTools({ tools, sandboxOn, onChange }: AdvancedToolsProps) {
  const toggle = (id: ToolName) => onChange({ ...tools, [id]: !tools[id] });
  return (
    <AdvancedSection title={ADVANCED_COPY.tools} line={ADVANCED_COPY.toolsLine} id="ck-adv-tools">
      <ul className="ck-potato-adv-list" aria-labelledby="ck-adv-tools">
        {toolRows(sandboxOn).map((row) => {
          const on = tools[row.id];
          return (
            <li key={row.id} className="ck-potato-adv-switch-row">
              <div className="ck-potato-adv-switch-text">
                <span id={`ck-adv-tool-${row.id}`} className="ck-potato-adv-switch-label">
                  {row.label}
                </span>
                <span className="ck-potato-adv-line">{row.line}</span>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={on}
                aria-labelledby={`ck-adv-tool-${row.id}`}
                className={`ck-potato-switch${on ? ' is-on' : ''}`}
                onClick={() => toggle(row.id)}
              >
                <span className="ck-potato-adv-sr">{on ? 'On' : 'Off'}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </AdvancedSection>
  );
}
