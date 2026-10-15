import { useLayoutEffect, useRef, useState } from 'react';

export interface SegOption<T extends string> {
  id: T;
  label: string;
}

/** Segmented control with a sliding active pill (CSS transition only). */
export function SegTabs<T extends string>({
  options,
  value,
  onChange,
  label,
  small,
}: {
  options: SegOption<T>[];
  value: T;
  onChange: (id: T) => void;
  label: string;
  small?: boolean;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const [pill, setPill] = useState<{ left: number; width: number } | null>(null);

  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const measure = () => {
      const el = root.querySelector<HTMLElement>('[aria-selected="true"]');
      if (el) setPill({ left: el.offsetLeft, width: el.offsetWidth });
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(root);
    return () => ro.disconnect();
  }, [value, options.length]);

  return (
    <div ref={rootRef} className={`ck-seg${small ? ' ck-seg--sm' : ''}`} role="tablist" aria-label={label}>
      {pill && <span className="ck-seg-pill" style={{ transform: `translateX(${pill.left}px)`, width: pill.width }} />}
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          role="tab"
          aria-selected={o.id === value}
          className="ck-seg-btn"
          onClick={() => onChange(o.id)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
