// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file packages/core/src/prompt-data.ts.
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: doc comment added; the function is unchanged.

/**
 * Escape untrusted text before it goes inside a prompt fence. `&` goes first, so an entity the
 * text already contains (`&lt;`) stays visible as text instead of becoming a delimiter. Quotes
 * are left alone: they are not delimiters in the fence.
 */
export function escapePromptData(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}
