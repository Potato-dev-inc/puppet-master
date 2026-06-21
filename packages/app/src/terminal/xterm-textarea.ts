/**
 * xterm's helper textarea must overlap its terminal host for IME and clipboard
 * paste to work in Tauri/WebView2. A 1px fixed textarea at viewport (220px, 80px)
 * breaks Ctrl+V in the sidebar orchestrator on Windows.
 */
export function configureDesktopXtermTextarea(
  host: HTMLElement,
  textarea: HTMLTextAreaElement,
): void {
  if (host.style.position === '' || host.style.position === 'static') {
    host.style.position = 'relative';
  }

  textarea.style.position = 'absolute';
  textarea.style.top = '0';
  textarea.style.left = '0';
  textarea.style.right = '0';
  textarea.style.bottom = '0';
  textarea.style.width = '100%';
  textarea.style.height = '100%';
  textarea.style.margin = '0';
  textarea.style.padding = '0';
  textarea.style.border = 'none';
  textarea.style.outline = 'none';
  textarea.style.resize = 'none';
  textarea.style.opacity = '0.01';
  textarea.style.zIndex = '10';
  textarea.style.caretColor = 'transparent';
  textarea.style.background = 'transparent';
}

export function normalizeTerminalPaste(text: string): string {
  return text.replace(/\r\n/g, '\r').replace(/\n/g, '\r');
}

export function isTerminalPasteShortcut(event: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey'>): boolean {
  return event.key.toLowerCase() === 'v' && (event.ctrlKey || event.metaKey) && !event.altKey;
}
