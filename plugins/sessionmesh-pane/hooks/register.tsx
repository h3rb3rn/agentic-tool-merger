import { atom, read, update } from "claude-code";
import type { EngineInterface, Register } from "claude-code";

import type { SessionRow } from "../types";

const PANE = "sessionmesh";
const FAMILIES = [
  "all",
  "codex",
  "claude-code",
  "opencode",
  "grok",
  "agy",
  "continue",
];
// The plugin's own bridge first, then a user-level registration of the same service.
const SERVERS = ["plugin:sessionmesh:sessionmesh", "sessionmesh"];
const LIMIT = 15;

// Change the tag when a row's fields change: it declines what an earlier version stored.
const sessions = atom(
  { plugin: "sessionmesh-pane", key: "sessions" } as const,
  [],
  {
    shape: "sessions-1",
  },
);
const filter = atom(
  { plugin: "sessionmesh-pane", key: "filter" } as const,
  "all",
);
const status = atom(
  { plugin: "sessionmesh-pane", key: "status" } as const,
  "Not loaded yet.",
);

/** Reads the list tool's structured result without trusting its shape. */
function parseRows(value: unknown): SessionRow[] {
  const items = (value as { items?: unknown } | null)?.items;
  if (!Array.isArray(items)) return [];
  return items.flatMap((item) => {
    const row = item as Record<string, unknown>;
    if (typeof row.native_session_id !== "string") return [];
    return [
      {
        id: row.native_session_id,
        tool: typeof row.tool_family === "string" ? row.tool_family : "?",
        title:
          typeof row.title === "string" && row.title ? row.title : "(untitled)",
        lastActivity:
          typeof row.last_activity_at === "string"
            ? row.last_activity_at
            : null,
        events: typeof row.event_count === "number" ? row.event_count : 0,
      },
    ];
  });
}

/** `2026-11-20T21:30:31Z` becomes `11-20 21:30`; anything else is shown as given. */
function shortTime(value: string | null): string {
  const match = value?.match(/^\d{4}-(\d{2}-\d{2})T(\d{2}:\d{2})/);
  return match ? `${match[1]} ${match[2]}` : (value ?? "—");
}

/** Asks SessionMesh for the newest sessions; a failure stays visible in the footer. */
async function refresh($: EngineInterface) {
  const family = await read($, filter);
  const args: Record<string, unknown> = { limit: LIMIT };
  if (family !== "all") args.tool_family = family;
  let lastError = "no SessionMesh server is connected";
  for (const server of SERVERS) {
    try {
      const result = await $.mcp.call(
        server,
        "sessionmesh_list_sessions",
        args,
      );
      if (result.isError) {
        lastError =
          result.content.map((block) => block.text ?? "").join(" ") ||
          "tool error";
        continue;
      }
      const rows = parseRows(result.structuredContent);
      await update($, sessions, () => rows);
      await update(
        $,
        status,
        () =>
          `${rows.length} sessions · ${shortTime(new Date().toISOString())}`,
      );
      return;
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
    }
  }
  await update($, status, () => `Could not load sessions: ${lastError}`);
}

export const register: Register = (on) => {
  on("session.start", async ($, e, next) => {
    await $.command.register({
      name: "sessionmesh-pane",
      description: "Show SessionMesh sessions of all coding agents in a pane",
    });
    void $.ui.open({ id: PANE, title: "SessionMesh" });
    void refresh($);

    return next(e);
  });

  on("command.run", { command: "sessionmesh-pane" }, async ($) => {
    await $.ui.open({ id: PANE, title: "SessionMesh" });
    await refresh($);

    return { text: "SessionMesh pane opened." };
  });

  on("turn.complete", async ($, e, next) => {
    void refresh($);

    return next(e);
  });

  on("ui.render", { component: "Pane", requestId: PANE }, async ($, e) => {
    const { Box, Text, Button } = $.ui.resolve(e);
    const rows = await read($, sessions);
    const active = await read($, filter);
    const line = await read($, status);
    const room = Math.max(1, (e.viewport?.rows ?? 24) - 8);

    return (
      <Box flexDirection="column">
        <Box flexDirection="row" flexWrap="wrap">
          {FAMILIES.map((family) => (
            <Button
              key={`f-${family}`}
              variant={family === active ? "primary" : undefined}
              onPress={async () => {
                await update($, filter, () => family);
                await refresh($);
              }}
            >
              {family}
            </Button>
          ))}
        </Box>
        {rows.length === 0 && <Text dimColor>No sessions found.</Text>}
        {rows.slice(0, room).map((row) => (
          <Button
            key={row.id}
            onPress={() =>
              $.prompt.fill({
                text: `/sessionmesh:continue ${row.id}`,
                mode: "replace",
              })
            }
          >
            <Text bold>{row.tool}</Text>
            <Text> {row.title}</Text>
            <Text dimColor>
              {" "}
              · {shortTime(row.lastActivity)} · {row.events} ev
            </Text>
          </Button>
        ))}
        <Box flexDirection="row">
          <Button key="refresh" onPress={() => refresh($)}>
            Refresh
          </Button>
          <Text dimColor wrap="truncate-end">
            {" "}
            {line}
          </Text>
        </Box>
        <Text dimColor>Click a session to prepare /sessionmesh:continue.</Text>
      </Box>
    );
  });
};
