import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

test("gitignore protects local secrets, settings, and runtime state", async () => {
  const gitignore = await readFile(
    path.join(repositoryRoot, ".gitignore"),
    "utf8",
  );
  const requiredRules = [
    ".env",
    ".env.*",
    "!.env.example",
    ".envrc",
    "/.sessionmesh/",
    "/secrets/",
    "/config.local.*",
    "/settings.local.*",
    "/credentials.*",
    "/api-token",
    "/sessionmesh.local.toml",
    "*.pem",
    "*.key",
    "*.token",
    "auth.json",
    "credentials.json",
  ];

  for (const rule of requiredRules) {
    assert.match(gitignore, new RegExp(`^${escapeRegExp(rule)}$`, "m"), rule);
  }
});

test("environment template contains no credential values", async () => {
  const template = await readFile(
    path.join(repositoryRoot, ".env.example"),
    "utf8",
  );
  const credentialNames = /(API_KEY|ACCESS_TOKEN|AUTH_TOKEN|PASSWORD|SECRET)=/;

  assert.doesNotMatch(template, credentialNames);
  assert.match(template, /^SESSIONMESH_ALLOW_NETWORK=false$/m);
  assert.match(template, /^SESSIONMESH_REDACTION_ENABLED=true$/m);
});

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
