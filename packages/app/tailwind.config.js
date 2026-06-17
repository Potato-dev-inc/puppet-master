/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        pm: {
          bg: 'rgb(var(--pm-bg) / <alpha-value>)',
          panel: 'rgb(var(--pm-panel) / <alpha-value>)',
          raised: 'rgb(var(--pm-raised) / <alpha-value>)',
          border: 'rgb(var(--pm-border) / <alpha-value>)',
          accent: 'rgb(var(--pm-accent) / <alpha-value>)',
          ok: 'rgb(var(--pm-ok) / <alpha-value>)',
          warn: 'rgb(var(--pm-warn) / <alpha-value>)',
          err: 'rgb(var(--pm-err) / <alpha-value>)',
          muted: 'rgb(var(--pm-muted) / <alpha-value>)',
          text: 'rgb(var(--pm-text) / <alpha-value>)',
        },
      },
    },
  },
  plugins: [],
};
