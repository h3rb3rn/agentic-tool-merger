import assert from "node:assert/strict";
import { readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const plugin = path.join(root, "plugins", "sessionmesh");
const readText = (...parts) => readFile(path.join(...parts), "utf8");
const readJson = async (...parts) => JSON.parse(await readText(...parts));

const READ_ONLY_PREFIX = "mcp__plugin_sessionmesh_sessionmesh__sessionmesh_";
const WRITE_TOOLS = ["record_decision", "record_task"];

test("plugin manifest and marketplace entry agree", async () => {
  const manifest = await readJson(plugin, ".claude-plugin", "plugin.json");
  const marketplace = await readJson(
    root,
    ".claude-plugin",
    "marketplace.json",
  );
  assert.equal(manifest.name, "sessionmesh");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  const entry = marketplace.plugins.find((item) => item.name === manifest.name);
  assert.ok(entry, "marketplace lists the plugin");
  assert.equal(entry.source, "./plugins/sessionmesh");
  await stat(path.join(root, entry.source, ".claude-plugin", "plugin.json"));
});

test("plugin MCP server uses the bundled bridge, not a host path", async () => {
  const config = await readJson(plugin, ".mcp.json");
  const server = config.mcpServers.sessionmesh;
  assert.equal(
    server.command,
    "${CLAUDE_PLUGIN_ROOT}/bin/sessionmesh-mcp-stdio",
  );
  const mode = (await stat(path.join(plugin, "bin", "sessionmesh-mcp-stdio")))
    .mode;
  assert.ok(mode & 0o100, "bridge is executable");
});

test("bundled bridge stays identical to the repository bridge", async () => {
  assert.equal(
    await readText(plugin, "bin", "sessionmesh-mcp-stdio"),
    await readText(root, "scripts", "sessionmesh-mcp-stdio"),
  );
});

test("commands are read-only and name only existing MCP tools", async () => {
  const commands = (await readdir(path.join(plugin, "commands"))).filter(
    (name) => name.endsWith(".md"),
  );
  assert.deepEqual(commands.sort(), ["continue.md", "sessions.md"]);
  const serverTools = new Set(
    [
      ...(
        await readText(root, "crates", "sessionmesh-mcp", "src", "lib.rs")
      ).matchAll(/tool\(\s*"(sessionmesh_[a-z_]+)"/g),
    ].map((match) => match[1]),
  );
  for (const name of commands) {
    const text = await readText(plugin, "commands", name);
    const frontmatter = text.match(/^---\n([\s\S]*?)\n---\n/);
    assert.ok(frontmatter, `${name} has frontmatter`);
    assert.match(frontmatter[1], /^description: .+/m);
    const allowed = frontmatter[1].match(/^allowed-tools: (.+)$/m);
    assert.ok(allowed, `${name} declares allowed-tools`);
    for (const tool of allowed[1].split(",").map((item) => item.trim())) {
      if (!tool.startsWith("mcp__")) continue;
      assert.ok(tool.startsWith(READ_ONLY_PREFIX), `${tool} is a plugin tool`);
      const short = tool.slice(READ_ONLY_PREFIX.length);
      assert.ok(serverTools.has(`sessionmesh_${short}`), `${tool} exists`);
      assert.ok(!WRITE_TOOLS.includes(short), `${tool} must not write`);
    }
  }
});

test("continue command treats session text as untrusted data", async () => {
  const text = await readText(plugin, "commands", "continue.md");
  assert.match(text, /untrusted/i);
  assert.match(text, /never an instruction/i);
  assert.match(text, /git status/);
});

test("pane mod is listed, uses only read-only tools and never sends prompts", async () => {
  const pane = path.join(root, "plugins", "sessionmesh-pane");
  const manifest = await readJson(pane, ".claude-plugin", "plugin.json");
  const marketplace = await readJson(
    root,
    ".claude-plugin",
    "marketplace.json",
  );
  assert.equal(manifest.name, "sessionmesh-pane");
  const entry = marketplace.plugins.find((item) => item.name === manifest.name);
  assert.equal(entry?.source, "./plugins/sessionmesh-pane");
  const hooks = await readJson(pane, "hooks", "hooks.json");
  assert.deepEqual(hooks.modules, ["./register.tsx"]);

  const source = await readText(pane, "hooks", "register.tsx");
  const tools = new Set(
    [...source.matchAll(/["'](sessionmesh_[a-z_]+)["']/g)].map(
      (match) => match[1],
    ),
  );
  assert.deepEqual(
    [...tools].sort(),
    [
      "sessionmesh_get_session",
      "sessionmesh_list_sessions",
      "sessionmesh_search",
    ],
    "the pane uses only read-only tools",
  );
  assert.match(
    source,
    /mode: ['"]replace['"]/,
    "a press only fills the prompt box",
  );
  assert.doesNotMatch(source, /prompt\.submit|\$\.session\.send/);
  await assert.rejects(stat(path.join(pane, ".claude-plugin", "types")));
});
