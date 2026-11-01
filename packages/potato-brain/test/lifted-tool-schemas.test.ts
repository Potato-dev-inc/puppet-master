// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, files packages/adapters/src/builtin-tools.ts and packages/adapters/src/browser-tools.ts (tool schemas; no upstream schema test exists).
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: focused node:test cases for the plain JSON-schema objects.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  BROWSER_ACT_KINDS,
  BROWSER_ACT_TOOL,
  BROWSER_NAVIGATE_TOOL,
  BROWSER_SNAPSHOT_TOOL,
  COMPUTER_ACTION_KINDS,
  COMPUTER_ACT_TOOL,
  COMPUTER_OBSERVE_TOOL,
  MAX_ACTIONS_PER_CALL,
  PAGE_AND_DESKTOP_TOOLS,
} from "../src/lifted/tool-schemas.ts";
import type { JsonSchema } from "../src/lifted/tool-schemas.ts";

type Branch = { properties: Record<string, JsonSchema>; required: string[] };

function actionBranches(): Branch[] {
  const parameters = COMPUTER_ACT_TOOL.function.parameters as {
    properties: { actions: { items: { oneOf: Branch[] } } };
  };
  return parameters.properties.actions.items.oneOf;
}

test("every tool is an OpenAI function tool with a name, description and object parameters", () => {
  assert.deepEqual(
    PAGE_AND_DESKTOP_TOOLS.map((tool) => tool.function.name),
    ["browser_navigate", "browser_snapshot", "browser_act", "computer_observe", "computer_act"],
  );
  for (const tool of PAGE_AND_DESKTOP_TOOLS) {
    assert.equal(tool.type, "function");
    assert.ok(tool.function.description.length > 0);
    assert.equal(tool.function.parameters["type"], "object");
  }
});

test("the tools survive a JSON round trip unchanged", () => {
  for (const tool of PAGE_AND_DESKTOP_TOOLS) {
    assert.deepEqual(JSON.parse(JSON.stringify(tool)), tool);
  }
});

test("computer_observe takes no arguments", () => {
  assert.deepEqual(COMPUTER_OBSERVE_TOOL.function.parameters, { type: "object", properties: {} });
});

test("computer_act takes 1 to 24 ordered actions", () => {
  const actions = (COMPUTER_ACT_TOOL.function.parameters as { properties: { actions: JsonSchema }; required: string[] })
    .properties.actions as { minItems: number; maxItems: number };
  assert.equal(actions.minItems, 1);
  assert.equal(actions.maxItems, MAX_ACTIONS_PER_CALL);
  assert.equal(MAX_ACTIONS_PER_CALL, 24);
  assert.deepEqual(
    (COMPUTER_ACT_TOOL.function.parameters as { required: string[] }).required,
    ["actions"],
  );
});

test("computer_act lists focus with an application, and the eight pointer, key and wait kinds", () => {
  assert.deepEqual([...COMPUTER_ACTION_KINDS], [
    "click",
    "move",
    "down",
    "up",
    "type",
    "key",
    "scroll",
    "wait",
    "focus",
  ]);
  const [focus, other] = actionBranches();
  assert.deepEqual(focus?.properties["kind"], { type: "string", enum: ["focus"] });
  assert.deepEqual(focus?.required, ["kind", "application"]);
  assert.deepEqual(other?.properties["kind"]?.["enum"], [
    "click",
    "move",
    "down",
    "up",
    "type",
    "key",
    "scroll",
    "wait",
  ]);
  assert.deepEqual(other?.required, ["kind"]);
  assert.deepEqual(other?.properties["direction"]?.["enum"], ["up", "down", "left", "right"]);
});

test("browser_act lists the seven kinds and a field hint for each action", () => {
  assert.deepEqual([...BROWSER_ACT_KINDS], [
    "click",
    "fill",
    "type",
    "upload",
    "fill_secret",
    "fill_card",
    "fill_pay_token",
  ]);
  const parameters = BROWSER_ACT_TOOL.function.parameters as {
    properties: { actions: { minItems: number; maxItems: number; items: { properties: Record<string, JsonSchema> } } };
  };
  assert.equal(parameters.properties.actions.minItems, 1);
  assert.equal(parameters.properties.actions.maxItems, MAX_ACTIONS_PER_CALL);
  assert.deepEqual(parameters.properties.actions.items.properties["kind"], {
    type: "string",
    enum: [...BROWSER_ACT_KINDS],
  });
  assert.ok(String(parameters.properties.actions.items.properties["field"]?.["description"]).includes("cvc"));
});

test("browser_navigate needs a url, and browser_snapshot takes no arguments", () => {
  assert.deepEqual(BROWSER_NAVIGATE_TOOL.function.parameters["required"], ["url"]);
  assert.deepEqual(BROWSER_SNAPSHOT_TOOL.function.parameters, { type: "object", properties: {} });
});
