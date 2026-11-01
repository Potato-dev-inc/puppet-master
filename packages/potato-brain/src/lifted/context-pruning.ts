// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file packages/adapters/src/context-pruning.ts.
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: local message types instead of @earendil-works/pi-ai;
// added dropRepeatedFrameImages (identical consecutive frames omit image bytes), applied by pruneForRebuild.

/** A content part the brain sends to a model. Only text and image parts are pruned. */
export type PromptPart =
  | { type: "text"; text: string }
  | { type: "image"; data: string; mimeType: string };

export type ToolResultMessage = {
  role: "toolResult";
  toolCallId: string;
  toolName: string;
  content: PromptPart[];
  /** Tool-specific details. A screenshot carries `frameId` here; a failure may carry `error`. */
  details?: unknown;
  isError: boolean;
  timestamp: number;
};

export type OtherMessage = {
  role: "user" | "assistant";
  content: string | PromptPart[];
  timestamp: number;
};

export type PruneMessage = ToolResultMessage | OtherMessage;

export class ModelImageLimitError extends Error {}

const DEFAULT_COMPUTER_SCREENSHOTS_TO_KEEP = 2;

/**
 * Tools whose result is a view of the current page or screen. Each new result supersedes the
 * earlier ones, so older results only cost context: a long browsing run otherwise re-sends every
 * snapshot it ever took on every model call.
 */
const PAGE_STATE_TOOL_NAMES = new Set([
  "browser_navigate",
  "browser_snapshot",
  "browser_act",
  "computer_observe",
  "computer_act",
]);
const DEFAULT_PAGE_STATE_RESULTS_TO_KEEP = 3;
/**
 * Only results that actually carry a page (a snapshot tree, an observation) are worth trimming
 * or counting. Navigation confirmations, action receipts and errors are a line or two: trimming
 * them saves nothing, and counting them would push real page state out of the kept set.
 */
const STALE_PAGE_STATE_MIN_CHARS = 1_000;
const STALE_PAGE_STATE_NOTE =
  "[Earlier page state trimmed to save context. Facts you still need from that page should already be in your notes or tracker; otherwise take a fresh snapshot.]";

/**
 * Replace all but the most recent large page-state tool results with a short note. The runtime
 * context policy applies this only when it rebuilds a run's transcript, never to a transcript the
 * model has already seen.
 */
export function pruneStalePageStateContext(
  messages: PruneMessage[],
  keep = DEFAULT_PAGE_STATE_RESULTS_TO_KEEP,
): PruneMessage[] {
  let remaining = Math.max(0, Math.floor(keep));
  let transformed: PruneMessage[] | undefined;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message?.role !== "toolResult" || !PAGE_STATE_TOOL_NAMES.has(message.toolName)) continue;
    // Failures are diagnostics, not fresh page state, even when their text is large.
    const returnedError = (message.details as { error?: unknown } | undefined)?.error;
    if (message.isError || (returnedError !== undefined && returnedError !== null)) continue;
    if (textLength(message) < STALE_PAGE_STATE_MIN_CHARS) continue;
    if (remaining > 0) {
      remaining -= 1;
      continue;
    }
    transformed ??= [...messages];
    transformed[index] = {
      ...message,
      content: [{ type: "text", text: STALE_PAGE_STATE_NOTE }],
    };
  }
  return transformed ?? messages;
}

function textLength(message: ToolResultMessage): number {
  let total = 0;
  for (const part of message.content) {
    if (part.type === "text") total += part.text.length;
  }
  return total;
}

/**
 * Keep the latest screenshots within an optional model image budget. The runtime context policy
 * applies this only when it rebuilds a run's transcript; between rebuilds screenshots accumulate
 * up to the model's limit, so a rebuild keeps at most the default two to leave room for more.
 */
export function pruneComputerScreenshotContext(
  messages: PruneMessage[],
  maxImagesPerPrompt?: number,
): PruneMessage[] {
  const imageLimit = normalizedImageLimit(maxImagesPerPrompt);
  let remaining = DEFAULT_COMPUTER_SCREENSHOTS_TO_KEEP;
  if (imageLimit !== undefined) {
    const nonScreenshotImages = messages.reduce(
      (count, message) =>
        isComputerScreenshotMessage(message) ? count : count + imagePartCount(message),
      0,
    );
    if (nonScreenshotImages > imageLimit) {
      throw new ModelImageLimitError(
        `The configured model image limit is ${imageLimit}, but the prompt contains ${nonScreenshotImages} non-screenshot images.`,
      );
    }
    remaining = Math.min(remaining, imageLimit - nonScreenshotImages);
  }
  let transformed: PruneMessage[] | undefined;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (!isComputerScreenshotMessage(message)) continue;
    const images = imagePartCount(message);
    if (remaining >= images) {
      remaining -= images;
      continue;
    }
    transformed ??= [...messages];
    transformed[index] = {
      ...message,
      content: message.content.filter((part) => part.type !== "image"),
    };
  }
  return transformed ?? messages;
}

/**
 * A screenshot whose frame is the same as the previous screenshot's frame, and that previous one
 * still carries its image, drops its image bytes. The text (frame metadata) stays. The earlier
 * image is still in the transcript, so the model keeps seeing the frame; the same rule the
 * computer tools apply when a live action leaves the screen unchanged.
 *
 * Run this after pruneComputerScreenshotContext: a duplicate whose earlier copy was already
 * pruned keeps its own image, so the frame is never left with no image at all.
 */
export function dropRepeatedFrameImages(messages: PruneMessage[]): PruneMessage[] {
  let transformed: PruneMessage[] | undefined;
  // Frame of the latest screenshot that still carries an image in this transcript.
  let imagedFrameId: string | undefined;
  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index];
    if (!isComputerScreenshotMessage(message)) continue;
    const frameId = frameIdOf(message);
    if (frameId === imagedFrameId) {
      transformed ??= [...messages];
      transformed[index] = {
        ...message,
        content: message.content.filter((part) => part.type !== "image"),
      };
      continue;
    }
    imagedFrameId = frameId;
  }
  return transformed ?? messages;
}

/** What a rebuild drops: stale page state, screenshots over the default or model limit, and repeated frame bytes. */
export function pruneForRebuild(messages: PruneMessage[], maxImagesPerPrompt?: number): PruneMessage[] {
  return dropRepeatedFrameImages(
    pruneComputerScreenshotContext(pruneStalePageStateContext(messages), maxImagesPerPrompt),
  );
}

/** A model's image limit as a whole number, or undefined when it has none or it isn't finite. */
export function normalizedImageLimit(maxImagesPerPrompt?: number): number | undefined {
  if (maxImagesPerPrompt === undefined || !Number.isFinite(maxImagesPerPrompt)) return undefined;
  return Math.max(0, Math.floor(maxImagesPerPrompt));
}

export function imagePartCount(message: PruneMessage): number {
  return partsOf(message).filter((part) => part.type === "image").length;
}

function partsOf(message: PruneMessage): PromptPart[] {
  return Array.isArray(message.content) ? message.content : [];
}

function frameIdOf(message: ToolResultMessage): string | undefined {
  const details = message.details;
  return details && typeof details === "object" && "frameId" in details
    ? (details as { frameId: string }).frameId
    : undefined;
}

function isComputerScreenshotMessage(
  message: PruneMessage | undefined,
): message is ToolResultMessage {
  if (message?.role !== "toolResult" || !message.content.some((part) => part.type === "image")) {
    return false;
  }
  return typeof frameIdOf(message) === "string";
}
