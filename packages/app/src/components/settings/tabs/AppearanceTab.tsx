// Settings > Appearance: sidebar width presets / custom width.
import { useEffect, useState } from 'react';
import {
  clampSidebarWidth,
  isSidebarWidthPreset,
  MAX_SIDEBAR_WIDTH,
  MIN_SIDEBAR_WIDTH,
  SIDEBAR_WIDTH_PRESET_LABELS,
  SIDEBAR_WIDTH_PRESETS,
} from '../../../lib/settings';
import { FieldInput, FieldSelect, InfoCard, SettingBlock, SettingsSection } from '../settings-ui';
import { AmbientSettings } from './AmbientSettings';
import type { SettingsTabContext } from './tab-context';

export function AppearanceTab({ ctx }: { ctx: SettingsTabContext }) {
  const { settings, setSettings, onSidebarWidthChange, currentSidebarWidth } = ctx;
  const storedWidth = clampSidebarWidth(settings.sidebar_width ?? 360);
  const liveWidth = clampSidebarWidth(currentSidebarWidth ?? storedWidth);
  const [useCustomWidth, setUseCustomWidth] = useState(() => !isSidebarWidthPreset(storedWidth));

  useEffect(() => {
    setUseCustomWidth(!isSidebarWidthPreset(storedWidth));
  }, [storedWidth]);

  const applySidebarWidth = (next: number) => {
    const clamped = clampSidebarWidth(next);
    setSettings({ ...settings, sidebar_width: clamped });
    onSidebarWidthChange?.(clamped);
  };

  const selectValue = useCustomWidth ? 'custom' : String(storedWidth);

  return (
    <SettingsSection title="Appearance" description="Tune interface density and terminal layout.">
      <SettingBlock label="Sidebar width" implemented description="Orchestrator sidebar width in the workspace (stored in settings.json). Drag the divider in the workspace to resize live.">
        <div className="flex flex-col gap-2">
          <p className="text-xs text-pm-muted">
            Current width:{' '}
            <span className="font-mono text-pm-text">{liveWidth}px</span>
            {useCustomWidth && isSidebarWidthPreset(liveWidth) && (
              <span className="ml-1">(custom preset)</span>
            )}
          </p>
          <FieldSelect
            value={selectValue}
            onChange={(e) => {
              if (e.target.value === 'custom') {
                setUseCustomWidth(true);
                return;
              }
              setUseCustomWidth(false);
              applySidebarWidth(Number(e.target.value));
            }}
          >
            {SIDEBAR_WIDTH_PRESETS.map((preset) => (
              <option key={preset} value={preset}>
                {SIDEBAR_WIDTH_PRESET_LABELS[preset]} ({preset}px)
              </option>
            ))}
            <option value="custom">Custom…</option>
          </FieldSelect>
          {useCustomWidth && (
            <div className="flex items-center gap-2">
              <FieldInput
                type="number"
                min={MIN_SIDEBAR_WIDTH}
                max={MAX_SIDEBAR_WIDTH}
                step={1}
                value={storedWidth}
                onChange={(e) => {
                  const value = Number(e.target.value);
                  if (!Number.isFinite(value)) return;
                  applySidebarWidth(value);
                }}
                className="font-mono"
              />
              <span className="shrink-0 text-xs text-pm-muted">px</span>
            </div>
          )}
        </div>
      </SettingBlock>
      <SettingBlock label="Default grid columns" implemented={false} description="Starting terminal grid when opening a workspace.">
        <FieldSelect value="2" disabled><option value="1">1 column</option><option value="2">2 columns</option><option value="3">3 columns</option></FieldSelect>
      </SettingBlock>
      <AmbientSettings />
      <InfoCard title="Design language" description="Warm accents are reserved for primary actions and active states." implemented={false} />
    </SettingsSection>
  );
}
