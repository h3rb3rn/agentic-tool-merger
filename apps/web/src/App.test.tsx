import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

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
    expect(api.getEvent).toHaveBeenCalledWith("event-1");
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
    expect(api.getEvent).toHaveBeenCalledWith("event-1");
    expect(api.getEvent).toHaveBeenCalledWith("event-2");
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
            { signal: "content_jaccard", similarity: 0.64 },
          ],
        },
      ]),
    });
    render(<App client={api} />);

    expect(await screen.findByText("78% match")).toBeInTheDocument();
    expect(screen.getByText("Workspace match: true")).toBeInTheDocument();
    expect(screen.getByText("Content similarity: 64%")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Link candidate" }));
    await waitFor(() =>
      expect(api.reviewCorrelationCandidate).toHaveBeenCalledWith(
        "candidate-1",
        "accept",
      ),
    );
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
