import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

const cases = [
  {
    schema: "config.schema.json",
    valid: "config.valid.json",
    invalid: "config.invalid.json",
  },
  {
    schema: "canonical-event.schema.json",
    valid: "canonical-event.valid.json",
    invalid: "canonical-event.invalid.json",
  },
  {
    schema: "tool-profile.schema.json",
    valid: "tool-profile.valid.json",
    invalid: "tool-profile.invalid.json",
  },
  {
    schema: "handoff.schema.json",
    valid: "handoff.valid.json",
    invalid: "handoff.invalid.json",
  },
];

async function readJson(relativePath) {
  const content = await readFile(
    path.join(repositoryRoot, relativePath),
    "utf8",
  );
  return JSON.parse(content);
}

function compileSchema(schema) {
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  addFormats(ajv);
  return ajv.compile(schema);
}

for (const schemaCase of cases) {
  test(`${schemaCase.schema} accepts its valid example`, async () => {
    const schema = await readJson(`schemas/${schemaCase.schema}`);
    const example = await readJson(`schemas/examples/${schemaCase.valid}`);
    const validate = compileSchema(schema);

    assert.equal(validate(example), true, JSON.stringify(validate.errors));
  });

  test(`${schemaCase.schema} rejects its incompatible example`, async () => {
    const schema = await readJson(`schemas/${schemaCase.schema}`);
    const example = await readJson(`schemas/examples/${schemaCase.invalid}`);
    const validate = compileSchema(schema);

    assert.equal(validate(example), false);
    assert.ok(validate.errors?.length);
  });
}
