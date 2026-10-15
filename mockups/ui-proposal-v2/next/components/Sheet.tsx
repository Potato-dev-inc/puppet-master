"use client";
import { useEffect } from "react";

export function Sheet(p: { title: string; body: string; confirm: string; cancel?: string; destructive?: boolean; onConfirm: () => void; onCancel: () => void }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && p.onCancel();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });
  return (
    <div className="scrim" onClick={p.onCancel}>
      <div className="sheet float" role="alertdialog" aria-modal="true" aria-label={p.title} onClick={(e) => e.stopPropagation()}>
        <h3>{p.title}</h3>
        <p>{p.body}</p>
        <div className="sheet-actions">
          <button className="btn" autoFocus onClick={p.onCancel}>{p.cancel ?? "Cancel"}</button>
          <button className="btn primary" onClick={p.onConfirm}>{p.confirm}</button>
        </div>
      </div>
    </div>
  );
}
