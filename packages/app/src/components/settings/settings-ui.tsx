// Row building blocks for the Settings window (macOS System Settings style). Everything here is a row
// inside a grouped inset list (`.st-list`); the panel supplies the list, the groups and the "Saved" tick.
// Monochrome: no colour, no badges. Styles live in styles/settings.css.
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from 'react';
import { Popup, Switch } from './controls';

/** A label with its one-line hint. */
function Label({ label, description }: { label: ReactNode; description?: ReactNode }) {
  return (
    <div className="st-label">
      <span>{label}</span>
      {description && <small>{description}</small>}
    </div>
  );
}

/** A row with the control on its right: a switch, a popup, a short field, a value, a button. */
export function SettingRow({
  label,
  description,
  children,
}: {
  label: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="st-row">
      <Label label={label} description={description} />
      {children !== undefined && <div className="st-control">{children}</div>}
    </div>
  );
}

/** A row with a body below its label: lists, forms, status panels, snippets. */
export function SettingBlock({
  label,
  description,
  children,
}: {
  label: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="st-row is-block">
      <Label label={label} description={description} />
      {children !== undefined && <div className="st-body">{children}</div>}
    </div>
  );
}

/** An on/off row. The change is saved by the caller the moment you flip it. */
export function SettingToggle({
  label,
  description,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  description?: ReactNode;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <SettingRow label={label} description={description}>
      <Switch checked={checked} onChange={onChange} label={label} disabled={disabled} />
    </SettingRow>
  );
}

/** A read-only value on the right of a label. */
export function InfoRow({ label, description, value }: { label: ReactNode; description?: ReactNode; value: ReactNode }) {
  return (
    <SettingRow label={label} description={description}>
      <span className="st-value">{value}</span>
    </SettingRow>
  );
}

export function FieldLabel({ children }: { children: ReactNode }) {
  return <label className="st-field-label">{children}</label>;
}

export function FieldInput(props: InputHTMLAttributes<HTMLInputElement>) {
  const { className, ...rest } = props;
  return <input {...rest} className={`st-field${className ? ` ${className.replace('font-mono', 'is-mono')}` : ''}`} />;
}

export function FieldSelect(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return <Popup {...props} />;
}

/** A button. `primary` is the single white action of a row; the default is the quiet grey one. */
export function Btn({
  primary,
  small,
  className,
  type = 'button',
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { primary?: boolean; small?: boolean }) {
  return <button {...rest} type={type} className={`st-btn${primary ? ' is-primary' : ''}${small ? ' is-small' : ''}${className ? ` ${className}` : ''}`} />;
}

/** A small error or status line under a control. */
export function Note({ children, role }: { children: ReactNode; role?: 'alert' | 'status' }) {
  return (
    <p className="st-note" role={role}>
      {children}
    </p>
  );
}

export function CodeBlock({ title, code }: { title: string; code: string }) {
  return (
    <SettingBlock label={title}>
      <div className="st-code">
        <Btn small onClick={() => void navigator.clipboard.writeText(code)}>
          Copy
        </Btn>
        <pre>{code}</pre>
      </div>
    </SettingBlock>
  );
}
