// A Labs switch. In a development build every Labs switch reads as on and is disabled, so it is clear why
// (`lib/dev-labs.ts`). Elsewhere it is the plain switch: `checked` and `onChange` as given.
import type { ReactNode } from 'react';
import { useDevLabsOverride } from '../../lib/dev-labs';
import { SettingToggle } from './settings-ui';

export function LabsSwitch({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description?: ReactNode;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  const forced = useDevLabsOverride();
  return <SettingToggle label={label} description={description} checked={forced || checked} onChange={onChange} disabled={forced} />;
}
