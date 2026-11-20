import { atom, read, update } from "claude-code";
import type { EngineInterface, Register } from "claude-code";

import type { SearchHit, SessionRow } from "../types";

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
const LIST_LIMIT = 15;
// The search tool returns the oldest matches first, so the pane reads a bounded
// number of pages and says so when more matches exist than it read.
const SEARCH_PAGE = 100;
const SEARCH_PAGES = 4;
const MAX_HIT_SESSIONS = 10;

// Change the tag when a row's fields change: it declines what an earlier version stored.
const sessions = atom(
  { plugin: "sessionmesh-pane", key: "sessions" } as const,
  [],
  { shape: "sessions-1" },
);
const hits = atom({ plugin: "sessionmesh-pane", key: "hits" } as const, [], {
  shape: "hits-1",
});
const filter = atom(
  { plugin: "sessionmesh-pane", key: "filter" } as const,
  "all",
);
const query = atom({ plugin: "sessionmesh-pane", key: "query" } as const, "");
const status = atom(
  { plugin: "sessionmesh-pane", key: "status" } as const,
  "Not loaded yet.",
);

type ToolOutcome = { ok: true; value: unknown } | { ok: false; error: string };

/** Calls a SessionMesh tool through the first connected server that answers. */
async function callTool(
  $: EngineInterface,
  tool: string,
  args: Record<string, unknown>,
): Promise<ToolOutcome> {
  let error = "no SessionMesh server is connected";
  for (const server of SERVERS) {
    try {
      const result = await $.mcp.call(server, tool, args);
      if (result.isError) {
        error =
          result.content.map((block) => block.text ?? "").join(" ") ||
          "tool error";
        continue;
      }
      return { ok: true, value: result.structuredContent };
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    }
  }
  return { ok: false, error };
}

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

/**
 * Turns what the person typed into a safe full-text query.
 *
 * The search tool hands its text to SQLite FTS5, where characters such as `-`,
 * `:` or `"` are syntax and fail the whole query. Every word is therefore quoted
 * as a phrase, and the quoted words must all match.
 */
function toFtsQuery(text: string): string {
  return text
    .split(/\s+/)
    .filter((word) => word.length > 0)
    .map((word) => `"${word.replaceAll('"', '""')}"`)
    .join(" ");
}

/** `2026-11-20T21:30:31Z` becomes `11-20 21:30`; anything else is shown as given. */
function shortTime(value: string | null): string {
  const match = value?.match(/^\d{4}-(\d{2}-\d{2})T(\d{2}:\d{2})/);
  return match ? `${match[1]} ${match[2]}` : (value ?? "—");
}

/** Asks SessionMesh for the newest sessions; a failure stays visible in the footer. */
async function refresh($: EngineInterface) {
  const family = await read($, filter);
  const args: Record<string, unknown> = { limit: LIST_LIMIT };
  if (family !== "all") args.tool_family = family;
  const outcome = await callTool($, "sessionmesh_list_sessions", args);
  if (!outcome.ok) {
    await update($, status, () => `Could not load sessions: ${outcome.error}`);
    return;
  }
  const rows = parseRows(outcome.value);
  await update($, sessions, () => rows);
  await update($, status, () => `${rows.length} sessions`);
}

/** Searches all sessions, groups the matching events by session, and names each. */
async function search($: EngineInterface, text: string) {
  const typed = text.trim();
  await update($, query, () => typed);
  if (typed === "") {
    await update($, hits, () => []);
    await update($, status, () => "Search cleared.");
    return;
  }
  await update($, status, () => `Searching "${typed}"…`);

  const groups = new Map<string, { matches: number; latest: string | null }>();
  let cursor: string | undefined;
  let hasMore = false;
  for (let page = 0; page < SEARCH_PAGES; page += 1) {
    const args: Record<string, unknown> = {
      query: toFtsQuery(typed),
      limit: SEARCH_PAGE,
    };
    if (cursor) args.cursor = cursor;
    const outcome = await callTool($, "sessionmesh_search", args);
    if (!outcome.ok) {
      await update($, hits, () => []);
      await update($, status, () => `Search failed: ${outcome.error}`);
      return;
    }
    const body = outcome.value as {
      items?: Array<Record<string, unknown>>;
      next_cursor?: string | null;
    } | null;
    for (const item of body?.items ?? []) {
      if (typeof item.native_session_id !== "string") continue;
      const stamp = typeof item.timestamp === "string" ? item.timestamp : null;
      const group = groups.get(item.native_session_id) ?? {
        matches: 0,
        latest: null,
      };
      group.matches += 1;
      if (stamp !== null && (group.latest === null || stamp > group.latest)) {
        group.latest = stamp;
      }
      groups.set(item.native_session_id, group);
    }
    cursor = body?.next_cursor ?? undefined;
    hasMore = cursor !== undefined;
    if (!hasMore) break;
  }

  const ranked = [...groups.entries()]
    .sort(
      ([, a], [, b]) =>
        b.matches - a.matches || (b.latest ?? "").localeCompare(a.latest ?? ""),
    )
    .slice(0, MAX_HIT_SESSIONS);
  const named = await Promise.all(
    ranked.map(async ([id, group]): Promise<SearchHit> => {
      const detail = await callTool($, "sessionmesh_get_session", {
        native_session_id: id,
        max_messages: 1,
      });
      const session = detail.ok
        ? ((detail.value as { session?: Record<string, unknown> } | null)
            ?.session ?? {})
        : {};
      return {
        id,
        tool:
          typeof session.tool_family === "string" ? session.tool_family : "?",
        title:
          typeof session.title === "string" && session.title
            ? session.title
            : "(untitled)",
        matches: group.matches,
        latest: group.latest,
      };
    }),
  );
  await update($, hits, () => named);
  const note = hasMore
    ? ` · only the oldest ${SEARCH_PAGE * SEARCH_PAGES} matches were read, narrow the search`
    : "";
  await update(
    $,
    status,
    () =>
      `${groups.size} sessions match "${typed}"${groups.size > named.length ? ` (top ${named.length} shown)` : ""}${note}`,
  );
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
    const { Box, Text, Button, Input } = $.ui.resolve(e);
    const rows = await read($, sessions);
    const found = await read($, hits);
    const active = await read($, filter);
    const typed = await read($, query);
    const line = await read($, status);
    const room = Math.max(1, (e.viewport?.rows ?? 24) - 9);
    const isSearching = typed !== "";

    return (
      <Box flexDirection="column">
        <Input
          key="search"
          label="Search"
          placeholder="words in any session, Enter to search"
          value={typed}
          submitLabel="search"
          onSubmit={(value) => search($, value)}
        />
        {isSearching ? (
          <Box flexDirection="column">
            <Button key="clear" onPress={() => search($, "")}>
              Clear search
            </Button>
            {found.length === 0 && <Text dimColor>No matching sessions.</Text>}
            {found.slice(0, room).map((hit) => (
              <Button
                key={`hit-${hit.id}`}
                onPress={() =>
                  $.prompt.fill({
                    text: `/sessionmesh:continue ${hit.id}`,
                    mode: "replace",
                  })
                }
              >
                <Text bold>{hit.tool}</Text>
                <Text> {hit.title}</Text>
                <Text dimColor>
                  {" "}
                  · {hit.matches} hits · {shortTime(hit.latest)}
                </Text>
              </Button>
            ))}
          </Box>
        ) : (
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
          </Box>
        )}
        <Box flexDirection="row">
          <Button key="refresh" onPress={() => refresh($)}>
            Refresh
          </Button>
          <Text dimColor wrap="truncate-end">
            {" "}
            {line}
          </Text>
        </Box>
        <Text dimColor>Press a session to prepare /sessionmesh:continue.</Text>
      </Box>
    );
  });
};
