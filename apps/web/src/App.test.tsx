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
      name: /session\/alpha/i,
    });
    session.focus();
    fireEvent.keyDown(session, { key: "Enter" });

    expect(await screen.findByText("Tool Call")).toBeInTheDocument();
    expect(screen.getByText("Tool Result")).toBeInTheDocument();
    expect(screen.getByLabelText("Tool activity group")).toBeInTheDocument();
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
    expect(await screen.findByText(/synthetic-secret/)).toBeInTheDocument();
    expect(api.getEvent).toHaveBeenCalledWith("event-1");
  });

  it("shows loading, empty, stale, and recoverable error states", async () => {
    const retry = vi
      .fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue({ items: [], next_cursor: null });
    render(<App client={client({ listSessions: retry })} />);

    expect(screen.getByRole("status")).toHaveTextContent("Loading sessions");
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
