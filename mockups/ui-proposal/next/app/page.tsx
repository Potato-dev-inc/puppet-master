import Link from "next/link";
import { ExampleBadge } from "@/components/ExampleBadge";

const routes = [
  ["/settings", "Settings", "6 sections, searchable rows, one save model, status rows"],
  ["/pane", "Pane window", "Glass title strip, live state chip, phone-sized banner"],
  ["/home", "Home + tray", "Attention-first overview with quick answers and tray menu"],
];

export default function Index() {
  return (
    <main style={{ maxWidth: 720, margin: "0 auto", padding: "64px 20px" }}>
      <ExampleBadge />
      <h1 style={{ font: "900 36px/1.05 var(--font)", letterSpacing: "-.025em", margin: "16px 0 6px" }}>Puppet Master, desktop UI proposal</h1>
      <p style={{ color: "var(--ink-2)" }}>Mock only. Nothing here talks to a backend.</p>
      <div style={{ display: "grid", gap: 12, marginTop: 28 }}>
        {routes.map(([h, t, d]) => (
          <Link key={h} href={h} className="glass" style={{ borderRadius: 18, padding: 20, textDecoration: "none" }}>
            <b style={{ fontSize: 18 }}>{t}</b> <span className="mono" style={{ color: "var(--ink-3)", fontSize: 12 }}>{h}</span>
            <div style={{ color: "var(--ink-2)" }}>{d}</div>
          </Link>
        ))}
      </div>
    </main>
  );
}
