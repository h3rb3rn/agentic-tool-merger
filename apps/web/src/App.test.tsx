import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";
import type { CanonicalEvent, EventSummary, SessionMeshClient } from "./api";
import { mergeChronologicalEvents } from "./timeline";

const sessions = [
  {
    id: "session/alpha",
    tool_family: "codex",
    surface: "cli",
    started_at: "2026-07-20T14:30:00Z",
    ended_at: null,
    thread_title: "Implement session timeline",
    event_count: 2,
    content_bytes: 2_048,
  },
];

const events: EventSummary[] = [
  {
    event_id: "event-1",
    native_session_id: "session/alpha",
    sequence: 1,
    timestamp: "2026-07-20T14:31:00Z",
    kind: "tool_call",
    cwd: "/workspace",
    branch: "main",
  },
  {
    event_id: "event-2",
    native_session_id: "session/alpha",
    sequence: 2,
    timestamp: "2026-07-20T14:31:01Z",
    kind: "tool_result",
    cwd: "/workspace",
    branch: "main",
  },
];

function client(overrides: Partial<SessionMeshClient> = {}): SessionMeshClient {
  return {
    getSession: vi.fn().mockImplementation((id: string) =>
      Promise.resolve({
        ...sessions[0],
        id,
        thread_title: id,
        tool_family: id.split(":")[0],
      }),
    ),
    listSessionTools: vi
      .fn()
      .mockResolvedValue([{ tool_family: "codex", session_count: 1 }]),
    listSessions: vi
      .fn()
      .mockResolvedValue({ items: sessions, next_cursor: null }),
    listEvents: vi.fn().mockResolvedValue({ items: events, next_cursor: null }),
    getEvent: vi.fn().mockResolvedValue({
      ...events[0],
      schema_version: "1.0",
      payload: { command: "printf synthetic-secret", output: "x".repeat(500) },
      provenance: { source_path: "/native/rollout.jsonl", source_offset: 12 },
    } as unknown as CanonicalEvent),
    searchEvents: vi.fn().mockResolvedValue([]),
    listCorrelationCandidates: vi.fn().mockResolvedValue([]),
    reviewCorrelationCandidate: vi.fn().mockResolvedValue(undefined),
    listGlobalSessions: vi.fn().mockResolvedValue([]),
    getGlobalSession: vi.fn(),
    createGlobalSession: vi.fn(),
    linkSession: vi.fn().mockResolvedValue(undefined),
    unlinkSession: vi.fn().mockResolvedValue(undefined),
    rejectSession: vi.fn().mockResolvedValue(undefined),
    subscribe: vi.fn().mockReturnValue(() => undefined),
    ...overrides,
  };
}

describe("Session timeline", () => {
  it("supports keyboard session selection and groups tool activity chronologically", async () => {
    render(<App client={client()} />);

    const session = await screen.findByRole("button", {
      name: /implement session timeline/i,
    });
    session.focus();
    fireEvent.keyDown(session, { key: "Enter" });

    expect(await screen.findByText("Tool Call")).toBeInTheDocument();
    expect(screen.getByText("Tool Result")).toBeInTheDocument();
    expect(screen.getByLabelText("Tool activity group")).toBeInTheDocument();
  });

  it("sorts and groups sessions by topic, date, and size", async () => {
    render(
      <App
        client={client({
          listSessions: vi.fn().mockResolvedValue({
            items: [
              sessions[0],
              {
                ...sessions[0],
                id: "session/beta",
                thread_title: "Analyze storage",
                started_at: "2026-07-21T10:00:00Z",
                content_bytes: 120_000,
              },
            ],
            next_cursor: null,
          }),
        })}
      />,
    );

    await screen.findByRole("button", { name: /Implement session timeline/ });
    fireEvent.change(screen.getByLabelText("Group by"), {
      target: { value: "none" },
    });
    fireEvent.change(screen.getByLabelText("Sort by"), {
      target: { value: "topic" },
    });
    const titles = screen
      .getAllByRole("button")
      .filter((button) => button.classList.contains("session"))
      .map((button) => button.querySelector("strong")?.textContent);
    expect(titles).toEqual(["Analyze storage", "Implement session timeline"]);

    fireEvent.change(screen.getByLabelText("Group by"), {
      target: { value: "size" },
    });
    expect(screen.getByText("Large (≥ 100 KB)")).toBeInTheDocument();
    expect(screen.getByText("Small (< 10 KB)")).toBeInTheDocument();
  });

  it("branches a large session list by tool and filters without exposing IDs", async () => {
    render(
      <App
        client={client({
          listSessions: vi.fn().mockResolvedValue({
            items: [
              sessions[0],
              {
                ...sessions[0],
                id: "opencode/private-id",
                tool_family: "opencode",
                thread_title: "Database migration",
              },
            ],
            next_cursor: null,
          }),
        })}
      />,
    );

    expect(await screen.findByText("codex")).toBeInTheDocument();
    expect(screen.getByText("opencode")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Filter sessions"), {
      target: { value: "migration" },
    });
    expect(
      await screen.findByRole("button", { name: /Database migration/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Implement session timeline/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("opencode/private-id")).not.toBeInTheDocument();
  });

  it("filters events and preserves unknown malformed kinds", async () => {
    const unknown = {
      ...events[0],
      event_id: "event-unknown",
      kind: "",
    };
    render(
      <App
        client={client({
          listEvents: vi
            .fn()
            .mockImplementation((_sessionId: string, kind?: string) =>
              Promise.resolve({
                items: kind ? [] : [unknown],
                next_cursor: null,
              }),
            ),
        })}
      />,
    );

    expect(await screen.findByText("Unknown event")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Event kind"), {
      target: { value: "assistant_message" },
    });
    expect(await screen.findByText("No matching events")).toBeInTheDocument();
  });

  it("requires an explicit warning acknowledgement before revealing payloads", async () => {
    const api = client();
    render(<App client={api} />);

    await screen.findByText("Tool Call");
    fireEvent.click(
      screen.getAllByRole("button", { name: "Reveal event data" })[0],
    );

    const dialog = screen.getByRole("dialog", { name: "Sensitive event data" });
    expect(dialog).toHaveTextContent("may contain secrets");
    expect(screen.queryByText(/synthetic-secret/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Reveal now" }));
    expect(
      (await screen.findAllByText(/synthetic-secret/)).length,
    ).toBeGreaterThan(0);
    expect(api.getEvent).toHaveBeenCalledWith(
      "event-1",
      expect.any(AbortSignal),
    );
  });

  it("loads human-readable content for the complete visible timeline", async () => {
    const api = client({
      getEvent: vi.fn().mockImplementation((eventId: string) =>
        Promise.resolve({
          ...events.find((event) => event.event_id === eventId),
          schema_version: "1.0",
          payload:
            eventId === "event-1"
              ? { command: "cargo test --workspace" }
              : { stdout: "98 tests passed", exit_code: 0 },
          provenance: {
            source_path: "/native/rollout.jsonl",
            source_offset: eventId === "event-1" ? 12 : 48,
          },
        }),
      ),
    });
    render(<App client={api} />);

    await screen.findByText("Tool Call");
    fireEvent.click(
      screen.getByRole("button", { name: "Show timeline content" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Show content now" }));

    expect(
      await screen.findByText("cargo test --workspace"),
    ).toBeInTheDocument();
    expect(await screen.findByText("98 tests passed")).toBeInTheDocument();
    expect(screen.getAllByText("/native/rollout.jsonl")).toHaveLength(2);
    expect(api.getEvent).toHaveBeenCalledWith(
      "event-1",
      expect.any(AbortSignal),
    );
    expect(api.getEvent).toHaveBeenCalledWith(
      "event-2",
      expect.any(AbortSignal),
    );
  });

  it("finds keyword content across sessions and identifies each agent tool", async () => {
    const api = client({
      searchEvents: vi.fn().mockResolvedValue([
        {
          event_id: "match-1",
          native_session_id: "claude:alpha",
          tool_family: "claude-code",
          kind: "assistant_message",
          timestamp: "2026-07-20T14:31:00Z",
          content: "Implemented canonical event migration",
          source_path: "/home/user/.claude/projects/alpha.jsonl",
        },
        {
          event_id: "match-2",
          native_session_id: "continue:beta",
          tool_family: "continue",
          kind: "user_message",
          timestamp: "2026-07-20T15:31:00Z",
          content: "Continue the canonical event migration",
          source_path: "/home/user/.continue/sessions/beta.json",
        },
      ]),
    });
    render(<App client={api} />);

    fireEvent.change(screen.getByLabelText("Search session content"), {
      target: { value: "canonical migration" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    expect(
      await screen.findByText("Implemented canonical event migration"),
    ).toBeInTheDocument();
    expect(screen.getByText("claude-code")).toBeInTheDocument();
    expect(screen.getByText("continue")).toBeInTheDocument();
    expect(screen.getByText("claude:alpha")).toBeInTheDocument();
    expect(api.searchEvents).toHaveBeenCalledWith("canonical migration");
  });

  it("explains and accepts an uncertain cross-tool correlation", async () => {
    const api = client({
      listSessions: vi.fn().mockResolvedValue({
        items: [
          {
            id: "agy:alpha",
            tool_family: "agy",
            surface: "cli",
            started_at: "2026-07-20T14:00:00Z",
            ended_at: "2026-07-20T14:30:00Z",
            thread_title: "Plan canonical event migration",
            event_count: 4,
            content_bytes: 2048,
          },
          {
            id: "opencode:beta",
            tool_family: "opencode",
            surface: "cli",
            started_at: "2026-07-20T14:40:00Z",
            ended_at: "2026-07-20T15:00:00Z",
            thread_title: "Implement event repository",
            event_count: 8,
            content_bytes: 4096,
          },
        ],
        next_cursor: null,
      }),
      listCorrelationCandidates: vi.fn().mockResolvedValue([
        {
          id: "candidate-1",
          left_native_session_id: "agy:alpha",
          right_native_session_id: "opencode:beta",
          target_global_session_id: "gs-1",
          score: 0.78,
          status: "pending",
          evidence: [
            { signal: "workspace", matched: true },
            {
              signal: "content_jaccard",
              similarity: 0.64,
              shared_terms: ["canonical", "event", "migration"],
            },
          ],
        },
      ]),
    });
    render(<App client={api} />);

    expect(await screen.findByText("78% match")).toBeInTheDocument();
    expect(
      await screen.findAllByText("Plan canonical event migration"),
    ).not.toHaveLength(0);
    expect(
      await screen.findAllByText("Implement event repository"),
    ).not.toHaveLength(0);
    expect(screen.getByText("agy · CLI")).toBeInTheDocument();
    expect(screen.getByText("opencode · CLI")).toBeInTheDocument();
    expect(screen.getByText("Workspace match: true")).toBeInTheDocument();
    expect(screen.getByText("Content similarity: 64%")).toBeInTheDocument();
    expect(
      screen.getByText("Shared keywords: canonical, event, migration"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Link candidate" }));
    await waitFor(() =>
      expect(api.reviewCorrelationCandidate).toHaveBeenCalledWith(
        "candidate-1",
        "accept",
      ),
    );
  });

  it("keeps a large correlation queue bounded and searchable", async () => {
    const candidates = Array.from({ length: 7 }, (_, index) => ({
      id: `candidate-${index}`,
      left_native_session_id: `agy:left-${index}`,
      right_native_session_id: `opencode:right-${index}`,
      target_global_session_id: "gs-1",
      score: 0.7 - index / 100,
      status: "pending" as const,
      evidence: [],
    }));
    const api = client({
      listCorrelationCandidates: vi.fn().mockResolvedValue(candidates),
    });
    render(<App client={api} />);

    expect(await screen.findByText("7 pending")).toBeInTheDocument();
    expect(screen.getAllByText(/% match$/)).toHaveLength(5);
    fireEvent.click(screen.getByRole("button", { name: "Show more matches" }));
    expect(screen.getAllByText(/% match$/)).toHaveLength(7);

    fireEvent.change(screen.getByLabelText("Filter correlation candidates"), {
      target: { value: "left-6" },
    });
    expect(screen.getAllByText(/% match$/)).toHaveLength(1);
    expect(screen.getByText("agy:left-6")).toBeInTheDocument();
  });

  it("shows loading, empty, stale, and recoverable error states", async () => {
    const retry = vi
      .fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue({ items: [], next_cursor: null });
    render(<App client={client({ listSessions: retry })} />);

    expect(screen.getByText("Loading sessions…")).toBeInTheDocument();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Sessions are temporarily unavailable",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByText("No native sessions found"),
    ).toBeInTheDocument();
  });
});

describe("live event reconciliation", () => {
  it("deduplicates reconnect replay and restores deterministic order", () => {
    const reordered = mergeChronologicalEvents(
      [events[1]],
      [events[0], events[1]],
    );

    expect(reordered.map((event) => event.event_id)).toEqual([
      "event-1",
      "event-2",
    ]);
  });

  it("adds a live event without a page reload", async () => {
    let publish: ((event: EventSummary) => void) | undefined;
    render(
      <App
        client={client({
          listEvents: vi
            .fn()
            .mockResolvedValue({ items: [events[0]], next_cursor: null }),
          subscribe: vi.fn((_lastId, onEvent) => {
            publish = onEvent;
            return () => undefined;
          }),
        })}
      />,
    );

    await screen.findByText("Tool Call");
    publish?.(events[1]);
    await waitFor(() =>
      expect(screen.getByText("Tool Result")).toBeInTheDocument(),
    );
  });
});

describe("bounded timeline pages", () => {
  it("keeps replay outside the visible page and navigates using the API cursor", async () => {
    let publish: ((event: EventSummary) => void) | undefined;
    const api = client({
      listEvents: vi.fn().mockImplementation((_sessionId, _kind, cursor) =>
        Promise.resolve({
          items: [cursor ? events[1] : events[0]],
          next_cursor: cursor ? null : "next-page",
        }),
      ),
      subscribe: vi.fn((_lastId, onEvent) => {
        publish = onEvent;
        return () => undefined;
      }),
    });
    render(<App client={api} />);
    await screen.findByText("Tool Call");
    act(() => {
      for (let index = 0; index < 500; index++)
        publish?.({ ...events[1], event_id: `replay-${index}` });
    });
    expect(screen.queryByText("Tool Result")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next events" }));
    expect(await screen.findByText("Tool Result")).toBeInTheDocument();
    expect(api.listEvents).toHaveBeenLastCalledWith(
      "session/alpha",
      undefined,
      "next-page",
    );
    fireEvent.click(screen.getByRole("button", { name: "Previous events" }));
    expect(await screen.findByText("Tool Call")).toBeInTheDocument();
  });

  it("bounds a live tail and keeps one subscription while replay arrives", async () => {
    let publish: ((event: EventSummary) => void) | undefined;
    const api = client({
      listEvents: vi
        .fn()
        .mockResolvedValue({ items: [events[0]], next_cursor: null }),
      subscribe: vi.fn((_lastId, onEvent) => {
        publish = onEvent;
        return () => undefined;
      }),
    });
    const { container } = render(<App client={api} />);
    await screen.findByText("Tool Call");
    act(() => {
      for (let index = 0; index < 500; index++)
        publish?.({ ...events[1], event_id: `tail-${index}` });
    });
    expect(container.querySelectorAll("article.event")).toHaveLength(200);
    expect(api.subscribe).toHaveBeenCalledTimes(1);
    expect(
      screen.getByRole("button", { name: "Refresh timeline for new events" }),
    ).toBeInTheDocument();
  });

  it("shows a recoverable error when individual content fails to load", async () => {
    const api = client({
      getEvent: vi
        .fn()
        .mockRejectedValueOnce(new Error("offline"))
        .mockResolvedValue({
          ...events[0],
          schema_version: "1.0",
          payload: { text: "Recovered message" },
          provenance: {},
        }),
    });
    render(<App client={api} />);
    await screen.findByText("Tool Call");
    fireEvent.click(
      screen.getAllByRole("button", { name: "Reveal event data" })[0],
    );
    fireEvent.click(screen.getByRole("button", { name: "Reveal now" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Event content could not be loaded",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Recovered message")).toBeInTheDocument();
  });
});

describe("saved API token recovery", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("clears a rejected token and reconnects with a replacement token", async () => {
    sessionStorage.setItem("api-token", "old-token");
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation((path, init) => {
        const valid = init.headers.Authorization === "Bearer replacement-token";
        return Promise.resolve(
          new Response(
            JSON.stringify(
              valid
                ? path.includes("native-sessions")
                  ? { items: [], next_cursor: null }
                  : []
                : { code: "unauthorized" },
            ),
            { status: valid ? 200 : 401 },
          ),
        );
      }),
    );
    render(<App />);
    expect(await screen.findByLabelText("Local API token")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("API token is invalid");
    expect(sessionStorage.getItem("api-token")).toBeNull();
    fireEvent.change(screen.getByLabelText("Local API token"), {
      target: { value: "replacement-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(
      await screen.findByText("No native sessions found"),
    ).toBeInTheDocument();
    expect(sessionStorage.getItem("api-token")).toBe("replacement-token");
  });
});

describe("session archive pages", () => {
  it("pages inside one agent while preserving its selection", async () => {
    const api = client({
      listSessionTools: vi
        .fn()
        .mockResolvedValue([{ tool_family: "grok", session_count: 205 }]),
      listSessions: vi.fn().mockImplementation((cursor) =>
        Promise.resolve({
          items: [
            {
              ...sessions[0],
              id: cursor ? "grok:later" : "grok:first",
              tool_family: "grok",
              thread_title: cursor
                ? "Later Grok conversation"
                : "Initial Grok conversation",
            },
          ],
          next_cursor: cursor ? null : "session-next",
        }),
      ),
    });
    render(<App client={api} />);
    await screen.findByRole("button", { name: /Initial Grok conversation/ });
    fireEvent.click(screen.getByRole("button", { name: "Next sessions" }));
    expect(
      await screen.findByRole("button", { name: /Later Grok conversation/ }),
    ).toBeInTheDocument();
    expect(api.listSessions).toHaveBeenLastCalledWith("session-next", "grok");
    fireEvent.click(screen.getByRole("button", { name: "Previous sessions" }));
    expect(
      await screen.findByRole("button", { name: /Initial Grok conversation/ }),
    ).toBeInTheDocument();
  });
});

it("renders normalized Grok text once while retaining canonical content evidence", async () => {
  const api = client({
    getEvent: vi.fn().mockResolvedValue({
      ...events[0],
      schema_version: "1.0",
      payload: {
        text: "Grok reply",
        content: { type: "text", text: "Grok reply" },
      },
      provenance: {},
    }),
  });
  const { container } = render(<App client={api} />);
  await screen.findByText("Tool Call");
  fireEvent.click(
    screen.getAllByRole("button", { name: "Reveal event data" })[0],
  );
  fireEvent.click(screen.getByRole("button", { name: "Reveal now" }));
  expect(await screen.findAllByText("Grok reply")).toHaveLength(1);
  expect(container.querySelectorAll(".event-content > div")).toHaveLength(1);
  expect(container.querySelector(".event-content details")).toHaveTextContent(
    '"type": "text"',
  );
});

it("keeps every agent tool visible while loading only the selected tool's sessions", async () => {
  const tools = ["agy", "claude-code", "codex", "continue", "grok", "opencode"];
  const api = client({
    listSessionTools: vi.fn().mockResolvedValue(
      tools.map((tool_family) => ({
        tool_family,
        session_count: tool_family === "grok" ? 205 : 40,
      })),
    ),
    listSessions: vi.fn().mockImplementation((_cursor, tool) =>
      Promise.resolve({
        items:
          tool === "opencode"
            ? [
                {
                  ...sessions[0],
                  id: "opencode:s",
                  tool_family: "opencode",
                  thread_title: "OpenCode conversation",
                },
              ]
            : sessions,
        next_cursor: tool === "opencode" ? null : "archive-next",
      }),
    ),
  });
  render(<App client={api} />);
  for (const tool of tools)
    expect(
      await screen.findByText(tool, {
        selector: ".agent-tool > strong",
      }),
    ).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "opencode (40 sessions)" }),
  );
  expect(
    await screen.findByRole("button", { name: /OpenCode conversation/ }),
  ).toBeInTheDocument();
  expect(api.listSessions).toHaveBeenLastCalledWith(undefined, "opencode");
  for (const tool of tools)
    expect(
      screen.getByText(tool, { selector: ".agent-tool > strong" }),
    ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Next sessions" })).toBeDisabled();
});

it("loads correlation metadata outside the selected agent without loading its full session page", async () => {
  const api = client({
    getSession: vi.fn().mockResolvedValue({
      ...sessions[0],
      id: "opencode:peer",
      tool_family: "opencode",
      thread_title: "OpenCode peer",
    }),
    listCorrelationCandidates: vi.fn().mockResolvedValue([
      {
        id: "cross-tool",
        left_native_session_id: sessions[0].id,
        right_native_session_id: "opencode:peer",
        target_global_session_id: null,
        score: 0.7,
        status: "pending",
        evidence: [],
      },
    ]),
  });
  render(<App client={api} />);
  expect(await screen.findByText("OpenCode peer")).toBeInTheDocument();
  expect(api.getSession).toHaveBeenCalledWith("opencode:peer");
  expect(api.listSessions).toHaveBeenLastCalledWith(undefined, "codex");
});
