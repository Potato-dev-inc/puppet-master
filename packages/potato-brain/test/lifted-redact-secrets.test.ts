// Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file packages/core/src/events.test.ts (the redactSecrets cases).
// Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
// see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: vitest replaced by node:test and node:assert;
// base64 (standard and url-safe) and URL-encoded cases added.

import { test } from "node:test";
import assert from "node:assert/strict";
import { redactSecrets } from "../src/lifted/redact-secrets.ts";

test("hides a longer value whole when a shorter one appears inside it", () => {
  assert.equal(redactSecrets("password=hunter123xyz", ["123", "hunter123xyz"]), "password=[redacted]");
  assert.equal(redactSecrets("a 123 b", ["123", "hunter123xyz", ""]), "a [redacted] b");
});

test("hides values that overlap without one containing the other", () => {
  assert.equal(redactSecrets("x=pre-hunter123xyz;", ["pre-hun", "hunter123xyz"]), "x=[redacted];");
  assert.equal(redactSecrets("abab abab", ["abab"]), "[redacted] [redacted]");
  assert.equal(redactSecrets("aaaaaa", ["aaaa"]), "[redacted]");
  assert.equal(
    redactSecrets("keyfake-keyfake-", ["fake-", "key"]),
    "[redacted][redacted][redacted][redacted]",
  );
});

test("hides each body line of a multi-line value when output splits or rejoins its lines", () => {
  const key =
    "-----BEGIN FAKE KEY-----\r\nAAAAfakeKeyLineOne01\nBBBBfakeKeyLineTwo02\nshort\n-----END FAKE KEY-----\n";
  assert.equal(redactSecrets(`key: ${key}`, [key]), "key: [redacted]");
  // An unquoted `echo $KEY` joins the lines with spaces.
  assert.equal(
    redactSecrets("-----BEGIN FAKE KEY----- AAAAfakeKeyLineOne01 BBBBfakeKeyLineTwo02 short -----END", [key]),
    "-----BEGIN FAKE KEY----- [redacted] [redacted] short -----END",
  );
  assert.equal(redactSecrets("tail: BBBBfakeKeyLineTwo02", [key]), "tail: [redacted]");
});

test("hides a standard base64 form of a secret", () => {
  const secret = "hunter2-deploy-token";
  const encoded = Buffer.from(secret, "utf8").toString("base64");
  assert.equal(redactSecrets(`echo ${encoded}`, [secret]), "echo [redacted]");
});

test("hides a url-safe base64 form of a secret, with or without padding", () => {
  // "~~~" is 0x7e 0x7e 0x7e: standard base64 "fn5+", url-safe "fn5-".
  assert.equal(Buffer.from("~~~").toString("base64"), "fn5+");
  assert.equal(Buffer.from("~~~").toString("base64url"), "fn5-");
  assert.equal(redactSecrets("a fn5- b", ["~~~"]), "a [redacted] b");
  assert.equal(redactSecrets("a fn5+ b", ["~~~"]), "a [redacted] b");
  // The url-safe form of this one has no padding.
  const secret = "ab";
  assert.equal(Buffer.from(secret).toString("base64url"), "YWI");
  assert.equal(redactSecrets("token=YWI&x", [secret]), "token=[redacted]&x");
});

test("hides a percent-encoded form of a secret, including form encoding and lower-case escapes", () => {
  const secret = "a b&c=d/e";
  assert.equal(redactSecrets("q=a%20b%26c%3Dd%2Fe", [secret]), "q=[redacted]");
  assert.equal(redactSecrets("q=a+b%26c%3Dd%2Fe", [secret]), "q=[redacted]");
  assert.equal(redactSecrets("q=a%20b%26c%3dd%2fe", [secret]), "q=[redacted]");
});

test("leaves text alone when no form of a secret is present", () => {
  assert.equal(redactSecrets("nothing to see here", ["hunter123xyz"]), "nothing to see here");
  assert.equal(redactSecrets("", ["hunter123xyz"]), "");
});
