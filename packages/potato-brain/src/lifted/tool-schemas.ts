// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, files packages/adapters/src/builtin-tools.ts (computer_observe, computer_act, browser_navigate, browser_snapshot, browser_act) and packages/adapters/src/browser-tools.ts (BROWSER_ACT_KINDS).
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: plain JSON-schema objects in OpenAI function-tool
// form (no zod, no @rakazo imports); computer_act adds maxItems 24 and minItems 1 on actions; the browser tool descriptions are unchanged.

export type JsonSchema = Record<string, unknown>;

export type OpenAiFunctionTool = {
  type: "function";
  function: { name: string; description: string; parameters: JsonSchema };
};

/** The desktop actions computer_act takes, in order. `focus` raises an application's window, launching it if absent. */
export const COMPUTER_ACTION_KINDS = [
  "click",
  "move",
  "down",
  "up",
  "type",
  "key",
  "scroll",
  "wait",
  "focus",
] as const;

/** Most actions one computer_act or browser_act call may batch. */
export const MAX_ACTIONS_PER_CALL = 24;

/** The action kinds browser_act takes. */
export const BROWSER_ACT_KINDS = [
  "click",
  "fill",
  "type",
  "upload",
  "fill_secret",
  "fill_card",
  "fill_pay_token",
] as const;

/** The card fields fill_card can type, from an approved purchase. */
export const PURCHASE_CARD_FIELDS = ["number", "expiry", "cvc", "postal_code"] as const;

const DIRECTIONS = ["up", "down", "left", "right"] as const;
const MOUSE_BUTTONS = ["left", "right"] as const;

export const COMPUTER_OBSERVE_TOOL: OpenAiFunctionTool = {
  type: "function",
  function: {
    name: "computer_observe",
    description:
      "Capture the current screen of this bot's computer. Returns frame metadata and an image. Observe before coordinate-based actions and whenever another actor may have changed the desktop.",
    parameters: { type: "object", properties: {} },
  },
};

export const COMPUTER_ACT_TOOL: OpenAiFunctionTool = {
  type: "function",
  function: {
    name: "computer_act",
    description:
      "Perform up to 24 ordered desktop actions on this bot's computer and return the resulting screen. Batch only predictable actions; stop before an outcome you need to inspect. Action kinds: click, move, down, up, type, key, scroll, wait, focus (raises the application's window, launching it if absent). Scroll turns the wheel at x,y; put them on the area to scroll, as for a click. Without them it scrolls wherever the pointer is. Direction up, down, left or right; amount 1-20 wheel notches, default 3.",
    parameters: {
      type: "object",
      properties: {
        actions: {
          type: "array",
          minItems: 1,
          maxItems: MAX_ACTIONS_PER_CALL,
          items: {
            oneOf: [
              {
                type: "object",
                properties: {
                  kind: { type: "string", enum: ["focus"] },
                  application: { type: "string", minLength: 1, pattern: "\\S" },
                  uri: { type: "string" },
                },
                required: ["kind", "application"],
              },
              {
                type: "object",
                properties: {
                  kind: {
                    type: "string",
                    enum: COMPUTER_ACTION_KINDS.filter((kind) => kind !== "focus"),
                  },
                  x: { type: "number" },
                  y: { type: "number" },
                  button: { type: "string", enum: [...MOUSE_BUTTONS] },
                  double: { type: "boolean" },
                  text: { type: "string" },
                  key: { type: "string" },
                  modifiers: { type: "array", items: { type: "string" } },
                  direction: { type: "string", enum: [...DIRECTIONS] },
                  amount: { type: "number" },
                  ms: { type: "number" },
                  application: { type: "string" },
                  uri: { type: "string" },
                },
                required: ["kind"],
              },
            ],
          },
        },
        observe: { type: "boolean" },
        settle_ms: { type: "number" },
      },
      required: ["actions"],
    },
  },
};

export const BROWSER_NAVIGATE_TOOL: OpenAiFunctionTool = {
  type: "function",
  function: {
    name: "browser_navigate",
    description:
      'Open a URL in the page browser on this bot\'s computer and return the page like browser_snapshot. Prefer this over pixel clicks for web pages. If the result includes fallback:"computer_act", use computer_act on the desktop browser instead.',
    parameters: {
      type: "object",
      properties: {
        url: { type: "string", description: "http(s) URL to open." },
      },
      required: ["url"],
    },
  },
};

export const BROWSER_SNAPSHOT_TOOL: OpenAiFunctionTool = {
  type: "function",
  function: {
    name: "browser_snapshot",
    description:
      "Capture an accessibility-style snapshot of the current page with element refs (e1, e2, …). Use refs with browser_act. A dropdown lists its options, and a line says when page text or controls were cut off. Frames follow the page, under their site. Prefer this over computer_observe for web pages. If loading is true, take another snapshot before deciding what the page shows. If fallback is computer_act, use the desktop tools instead.",
    parameters: { type: "object", properties: {} },
  },
};

export const BROWSER_ACT_TOOL: OpenAiFunctionTool = {
  type: "function",
  function: {
    name: "browser_act",
    description: `Click or fill page elements by ref from browser_snapshot (kinds: ${BROWSER_ACT_KINDS.join(", ")}). click is a real mouse click, and fill on a dropdown takes one of its options' visible labels or values. The result shows the page after the action, and a snapshot covers the whole page, so scrolling is rarely needed. upload attaches your files to a "file upload" element; use it instead of clicking upload buttons. fill_secret types one field of a login saved with request_secret, by its label, only on the site it was saved for (in a frame: the frame's site); you never see the value. fill_card types an approved purchase's card details, only on its merchant site; leave out ref to find the field, including in a payment frame. fill_pay_token hands an approved purchase's pay token to its checkout; no ref. type presses keys like a keyboard and replaces the field's text; use it for code boxes and fields that ignore fill. For a code split into one box per digit, type the whole code once into the first box, and to correct it type it again. Prefer this over computer_act for web pages. If the result includes fallback:"computer_act", use computer_act instead.`,
    parameters: {
      type: "object",
      properties: {
        actions: {
          type: "array",
          minItems: 1,
          maxItems: MAX_ACTIONS_PER_CALL,
          items: {
            type: "object",
            properties: {
              kind: { type: "string", enum: [...BROWSER_ACT_KINDS] },
              ref: {
                type: "string",
                description: "Element ref from browser_snapshot. Optional for fill_card.",
              },
              text: {
                type: "string",
                description: "Text for fill or type (type replaces the field's text).",
              },
              paths: {
                type: "array",
                items: { type: "string" },
                description: "Files to attach, for upload.",
              },
              secret: { type: "string", description: "Saved login name for fill_secret." },
              purchase: {
                type: "string",
                description: "Approved purchase id for fill_card or fill_pay_token.",
              },
              field: {
                type: "string",
                description: `The login field's label for fill_secret (username and password also name a login's first visible and first hidden field); ${PURCHASE_CARD_FIELDS.join(", ")} for fill_card.`,
              },
            },
            required: ["kind"],
          },
        },
      },
      required: ["actions"],
    },
  },
};

/** The five page and desktop tools. */
export const PAGE_AND_DESKTOP_TOOLS: readonly OpenAiFunctionTool[] = [
  BROWSER_NAVIGATE_TOOL,
  BROWSER_SNAPSHOT_TOOL,
  BROWSER_ACT_TOOL,
  COMPUTER_OBSERVE_TOOL,
  COMPUTER_ACT_TOOL,
];
