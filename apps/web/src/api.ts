export interface Page<T> {
  items: T[];
  next_cursor: string | null;
}

export interface NativeSession {
  id: string;
  tool_family: string;
  surface: string;
  started_at: string | null;
  ended_at: string | null;
}

export interface EventSummary {
  event_id: string;
  native_session_id: string;
  sequence: number;
  timestamp: string;
  kind: string;
  cwd: string | null;
  branch: string | null;
}

export interface CanonicalEvent extends EventSummary {
  schema_version: string;
  payload: Record<string, unknown>;
  provenance: Record<string, unknown>;
}

export interface SearchResult {
  event_id: string;
  native_session_id: string;
  tool_family: string;
  kind: string;
  timestamp: string;
  content: string;
  source_path: string;
}

export interface GlobalSession {
  id: string;
  objective: string;
  created_at: string;
  updated_at: string;
}

export interface GlobalSessionDetail {
  session: GlobalSession;
  members: Array<{
    native_session_id: string;
    confidence: number;
    correlation_version: string;
    manual_state: string | null;
  }>;
  audit: Array<{
    id: number;
    native_session_id: string;
    action: string;
    actor: string;
    reason: string | null;
    created_at: string;
  }>;
}

export interface SessionMeshClient {
  listSessions(): Promise<Page<NativeSession>>;
  listEvents(sessionId: string, kind?: string): Promise<Page<EventSummary>>;
  getEvent(eventId: string): Promise<CanonicalEvent>;
  searchEvents(query: string): Promise<SearchResult[]>;
  listGlobalSessions(): Promise<GlobalSession[]>;
  getGlobalSession(id: string): Promise<GlobalSessionDetail>;
  createGlobalSession(objective: string): Promise<GlobalSession>;
  linkSession(globalId: string, nativeId: string): Promise<void>;
  unlinkSession(globalId: string, nativeId: string): Promise<void>;
  rejectSession(globalId: string, nativeId: string): Promise<void>;
  subscribe(
    lastEventId: string | undefined,
    onEvent: (event: EventSummary) => void,
    onConnectionChange?: (connected: boolean) => void,
  ): () => void;
}

interface ErrorEnvelope {
  code?: string;
  message?: string;
}

/**
 * Creates the authenticated browser client. Streaming uses fetch instead of
 * EventSource because native EventSource cannot send the local bearer token.
 */
export function createApiClient(token: string): SessionMeshClient {
  const request = async <T>(path: string, init?: RequestInit): Promise<T> => {
    const response = await fetch(path, {
      ...init,
      headers: {
        ...init?.headers,
        Authorization: `Bearer ${token}`,
      },
    });
    if (!response.ok) {
      const error = (await response.json().catch(() => ({}))) as ErrorEnvelope;
      throw new Error(error.message ?? `Request failed (${response.status})`);
    }
    return (await response.json()) as T;
  };

  return {
    listSessions: () => request("/api/v1/native-sessions?limit=200"),
    listEvents: (sessionId, kind) => {
      const query = new URLSearchParams({
        native_session_id: sessionId,
        limit: "200",
      });
      if (kind) query.set("kind", kind);
      return request(`/api/v1/events?${query.toString()}`);
    },
    getEvent: (eventId) =>
      request(`/api/v1/events/${encodeURIComponent(eventId)}`),
    searchEvents: (query) =>
      request(
        `/api/v1/events/search?${new URLSearchParams({ query, limit: "100" }).toString()}`,
      ),
    listGlobalSessions: () => request("/api/v1/global-sessions"),
    getGlobalSession: (id) =>
      request(`/api/v1/global-sessions/${encodeURIComponent(id)}`),
    createGlobalSession: (objective) =>
      request("/api/v1/global-sessions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ objective }),
      }),
    linkSession: (globalId, nativeId) =>
      request(
        `/api/v1/global-sessions/${encodeURIComponent(globalId)}/members`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ native_session_id: nativeId }),
        },
      ),
    unlinkSession: (globalId, nativeId) =>
      request(
        `/api/v1/global-sessions/${encodeURIComponent(globalId)}/members/${encodeURIComponent(nativeId)}`,
        { method: "DELETE" },
      ),
    rejectSession: (globalId, nativeId) =>
      request("/api/v1/correlations/reject", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          global_session_id: globalId,
          native_session_id: nativeId,
        }),
      }),
    subscribe: (lastEventId, onEvent, onConnectionChange) => {
      const controller = new AbortController();
      void consumeEventStream(
        token,
        lastEventId,
        controller.signal,
        onEvent,
        onConnectionChange,
      );
      return () => controller.abort();
    },
  };
}

async function consumeEventStream(
  token: string,
  lastEventId: string | undefined,
  signal: AbortSignal,
  onEvent: (event: EventSummary) => void,
  onConnectionChange?: (connected: boolean) => void,
): Promise<void> {
  let resumeId = lastEventId;
  let retryDelay = 500;
  while (!signal.aborted) {
    try {
      const headers: Record<string, string> = {
        Accept: "text/event-stream",
        Authorization: `Bearer ${token}`,
      };
      if (resumeId) headers["Last-Event-ID"] = resumeId;
      const response = await fetch("/api/v1/events/stream", {
        headers,
        signal,
      });
      if (!response.ok || !response.body) throw new Error("stream unavailable");
      onConnectionChange?.(true);
      retryDelay = 500;
      resumeId = await readSse(response.body, resumeId, onEvent, signal);
    } catch {
      if (signal.aborted) return;
      onConnectionChange?.(false);
      await abortableDelay(retryDelay, signal);
      retryDelay = Math.min(retryDelay * 2, 10_000);
    }
  }
}

async function readSse(
  body: ReadableStream<Uint8Array>,
  resumeId: string | undefined,
  onEvent: (event: EventSummary) => void,
  signal: AbortSignal,
): Promise<string | undefined> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  while (!signal.aborted) {
    const result = await reader.read();
    if (result.done) break;
    buffer += decoder.decode(result.value, { stream: true });
    let boundary = buffer.indexOf("\n\n");
    while (boundary >= 0) {
      const block = buffer.slice(0, boundary);
      buffer = buffer.slice(boundary + 2);
      const id = block
        .split("\n")
        .find((line) => line.startsWith("id:"))
        ?.slice(3)
        .trim();
      const data = block
        .split("\n")
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).trimStart())
        .join("\n");
      if (data) {
        onEvent(JSON.parse(data) as EventSummary);
        resumeId = id ?? resumeId;
      }
      boundary = buffer.indexOf("\n\n");
    }
  }
  return resumeId;
}

function abortableDelay(
  milliseconds: number,
  signal: AbortSignal,
): Promise<void> {
  return new Promise((resolve) => {
    const timeout = window.setTimeout(resolve, milliseconds);
    signal.addEventListener(
      "abort",
      () => {
        window.clearTimeout(timeout);
        resolve();
      },
      { once: true },
    );
  });
}
