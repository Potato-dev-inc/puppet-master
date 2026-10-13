import "./globals.css";
import type { ReactNode } from "react";

export const metadata = { title: "Puppet Master UI proposal (mock)" };

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <head>
        <meta name="color-scheme" content="dark light" />
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet" />
      </head>
      <body>{children}</body>
    </html>
  );
}
