import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

const allowedWorkspaceDependencies = {
  "sessionmesh-acp": ["sessionmesh-core"],
  "sessionmesh-adapter-sdk": ["sessionmesh-core"],
  "sessionmesh-api": [
    "sessionmesh-core",
    "sessionmesh-handoff",
    "sessionmesh-storage",
  ],
  "sessionmesh-cli": ["sessionmesh-core"],
  "sessionmesh-core": [],
  "sessionmesh-correlator": ["sessionmesh-core"],
  "sessionmesh-daemon": [
    "sessionmesh-api",
    "sessionmesh-core",
    "sessionmesh-handoff",
    "sessionmesh-ingest",
    "sessionmesh-storage",
  ],
  "sessionmesh-handoff": ["sessionmesh-core"],
  "sessionmesh-ingest": [
    "sessionmesh-adapter-sdk",
    "sessionmesh-core",
    "sessionmesh-storage",
  ],
  "sessionmesh-mcp": [
    "sessionmesh-core",
    "sessionmesh-correlator",
    "sessionmesh-handoff",
    "sessionmesh-storage",
  ],
  "sessionmesh-storage": ["sessionmesh-core"],
};

test("workspace crates obey the documented dependency direction", async () => {
  for (const [crateName, allowed] of Object.entries(
    allowedWorkspaceDependencies,
  )) {
    const manifest = await readFile(
      path.join(repositoryRoot, "crates", crateName, "Cargo.toml"),
      "utf8",
    );
    const dependencySection =
      manifest.match(/\[dependencies\]\n([\s\S]*?)(?=\n\[|$)/)?.[1] ?? "";
    const actual = [
      ...dependencySection.matchAll(/^(sessionmesh-[\w-]+)\s*=/gm),
    ]
      .map((match) => match[1])
      .sort();

    assert.deepEqual(actual, [...allowed].sort(), crateName);
  }
});
