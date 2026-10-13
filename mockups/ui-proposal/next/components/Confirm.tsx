"use client";
import type { ReactNode } from "react";

export function Confirm(p: { title: string; body: ReactNode; confirmLabel: string; cancelLabel?: string; danger?: boolean; onConfirm: () => void; onCancel: () => void }) {
  return (
    <div className="modal-bg" role="dialog" aria-modal="true" onClick={p.onCancel}>
      <div className="modal glass" onClick={(e) => e.stopPropagation()}>
        <h3>{p.title}</h3>
        <p>{p.body}</p>
        <div className="row">
          <button className="cap ghost" autoFocus onClick={p.onCancel}>{p.cancelLabel ?? "Cancel"}</button>
          <button className={`cap ${p.danger ? "danger" : "go"}`} onClick={p.onConfirm}>{p.confirmLabel}</button>
        </div>
      </div>
    </div>
  );
}
