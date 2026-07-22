import {
  QueryClient,
  QueryClientProvider,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { FormEvent, useEffect, useMemo, useState } from "react";

import "./styles.css";
import {
  type CanonicalEvent,
  type EventSummary,
  type NativeSession,
  type SessionMeshClient,
  createApiClient,
} from "./api";
import { eventLabel, mergeChronologicalEvents } from "./timeline";

export interface AppProps {
  client?: SessionMeshClient;
}

export function App({ client }: AppProps) {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: { queries: { retry: false, staleTime: 5_000 } },
      }),
  );
  const [token, setToken] = useState(
    () => sessionStorage.getItem("api-token") ?? "",
  );
  const activeClient = useMemo(
    () => client ?? (token ? createApiClient(token) : undefined),
    [client, token],
  );

  if (!activeClient) {
    return <TokenSetup onSave={setToken} />;
  }
  return (
    <QueryClientProvider client={queryClient}>
      <Timeline client={activeClient} />
    </QueryClientProvider>
  );
}

function TokenSetup({ onSave }: { onSave: (token: string) => void }) {
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const token = String(data.get("token") ?? "").trim();
    if (token) {
      sessionStorage.setItem("api-token", token);
      onSave(token);
    }
  };
  return (
    <main className="connect-shell">
      <p className="eyebrow">LOCAL AUTHENTICATION</p>
      <h1>SessionMesh</h1>
      <p>Enter the token from your SessionMesh home. It remains in this tab.</p>
      <form onSubmit={submit}>
        <label htmlFor="token">Local API token</label>
        <input
          id="token"
          name="token"
          type="password"
          autoComplete="off"
          required
        />
        <button type="submit">Connect</button>
      </form>
    </main>
  );
}

type SessionSort = "topic" | "date" | "size";
type SessionGroup = "none" | SessionSort;

function groupSessions(
  sessions: NativeSession[],
  sort: SessionSort,
  group: SessionGroup,
) {
  const sorted = [...sessions].sort((left, right) => {
    if (sort === "topic") {
      return left.thread_title.localeCompare(right.thread_title);
    }
    if (sort === "size") return right.content_bytes - left.content_bytes;
    return (right.ended_at ?? right.started_at ?? "").localeCompare(
      left.ended_at ?? left.started_at ?? "",
    );
  });
  const groups = new Map<string, NativeSession[]>();
  for (const session of sorted) {
    const label = sessionGroupLabel(session, group);
    groups.set(label, [...(groups.get(label) ?? []), session]);
  }
  return [...groups].map(([label, groupedSessions]) => ({
    label,
    sessions: groupedSessions,
  }));
}

function sessionGroupLabel(session: NativeSession, group: SessionGroup) {
  if (group === "topic") return session.thread_title;
  if (group === "date") {
    const timestamp = session.ended_at ?? session.started_at;
    return timestamp ? timestamp.slice(0, 10) : "Unknown date";
  }
  if (group === "size") {
    if (session.content_bytes < 10_000) return "Small (< 10 KB)";
    if (session.content_bytes < 100_000) return "Medium (10–100 KB)";
    return "Large (≥ 100 KB)";
  }
  return "All sessions";
}

function formatBytes(bytes: number) {
  if (bytes < 1_000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${(bytes / 1_000).toFixed(1)} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

function Timeline({ client }: { client: SessionMeshClient }) {
  const cache = useQueryClient();
  const [selectedId, setSelectedId] = useState(
    () => new URLSearchParams(window.location.search).get("session") ?? "",
  );
  const [kind, setKind] = useState("");
  const [searchText, setSearchText] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [showContent, setShowContent] = useState(false);
  const [confirmingContent, setConfirmingContent] = useState(false);
  const [connected, setConnected] = useState(true);
  const [sessionSort, setSessionSort] = useState<"topic" | "date" | "size">(
    "date",
  );
  const [sessionGroup, setSessionGroup] = useState<
    "none" | "topic" | "date" | "size"
  >("none");
  const sessions = useQuery({
    queryKey: ["sessions"],
    queryFn: () => client.listSessions(),
  });
  const effectiveId = selectedId || sessions.data?.items[0]?.id || "";
  const selectedSession = sessions.data?.items.find(
    (session) => session.id === effectiveId,
  );
  const sessionGroups = useMemo(
    () => groupSessions(sessions.data?.items ?? [], sessionSort, sessionGroup),
    [sessionGroup, sessionSort, sessions.data?.items],
  );
  const events = useQuery({
    queryKey: ["events", effectiveId, kind],
    queryFn: () => client.listEvents(effectiveId, kind || undefined),
    enabled: Boolean(effectiveId),
  });
  const search = useQuery({
    queryKey: ["event-search", searchQuery],
    queryFn: () => client.searchEvents(searchQuery),
    enabled: Boolean(searchQuery),
  });

  useEffect(() => {
    if (!effectiveId) return;
    const url = new URL(window.location.href);
    url.searchParams.set("session", effectiveId);
    window.history.replaceState(null, "", url);
  }, [effectiveId]);

  useEffect(() => {
    if (!effectiveId) return;
    const lastId = events.data?.items.at(-1)?.event_id;
    return client.subscribe(
      lastId,
      (incoming) => {
        if (incoming.native_session_id !== effectiveId) return;
        cache.setQueryData(
          ["events", effectiveId, kind],
          (
            page:
              | { items: EventSummary[]; next_cursor: string | null }
              | undefined,
          ) => ({
            items: mergeChronologicalEvents(page?.items ?? [], [incoming]),
            next_cursor: page?.next_cursor ?? null,
          }),
        );
      },
      setConnected,
    );
  }, [cache, client, effectiveId, events.data?.items, kind]);

  return (
    <main className="app-shell">
      <header>
        <div>
          <p className="eyebrow">LOCAL-FIRST SESSION OBSERVABILITY</p>
          <h1>SessionMesh</h1>
        </div>
        <span className={connected ? "connection live" : "connection stale"}>
          {connected ? "Live" : "Reconnecting — showing stale data"}
        </span>
      </header>
      <div className="workspace">
        <aside aria-label="Native sessions">
          <h2>Sessions</h2>
          <div className="session-controls">
            <label>
              Sort by
              <select
                value={sessionSort}
                onChange={(event) =>
                  setSessionSort(
                    event.target.value as "topic" | "date" | "size",
                  )
                }
              >
                <option value="date">Date</option>
                <option value="topic">Topic</option>
                <option value="size">Size</option>
              </select>
            </label>
            <label>
              Group by
              <select
                value={sessionGroup}
                onChange={(event) =>
                  setSessionGroup(
                    event.target.value as "none" | "topic" | "date" | "size",
                  )
                }
              >
                <option value="none">No groups</option>
                <option value="topic">Topic</option>
                <option value="date">Date</option>
                <option value="size">Size</option>
              </select>
            </label>
          </div>
          {sessions.isPending && <p role="status">Loading sessions…</p>}
          {sessions.isError && (
            <ErrorState
              message="Sessions are temporarily unavailable"
              retry={sessions.refetch}
            />
          )}
          {sessions.data?.items.length === 0 && <p>No native sessions found</p>}
          <nav>
            {sessionGroups.map((group) => (
              <section className="session-group" key={group.label}>
                {sessionGroup !== "none" && <h3>{group.label}</h3>}
                {group.sessions.map((session) => (
                  <button
                    className={
                      session.id === effectiveId ? "session active" : "session"
                    }
                    key={session.id}
                    title={`Native session: ${session.id}`}
                    onClick={() => setSelectedId(session.id)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        setSelectedId(session.id);
                      }
                    }}
                  >
                    <strong>{session.thread_title}</strong>
                    <span>
                      {session.tool_family} ·{" "}
                      {formatBytes(session.content_bytes)} ·{" "}
                      {session.event_count} events
                    </span>
                  </button>
                ))}
              </section>
            ))}
          </nav>
          <GlobalSessionReview client={client} nativeSessionId={effectiveId} />
          <CorrelationReview client={client} />
        </aside>
        <section className="timeline-panel" aria-labelledby="timeline-heading">
          <form
            className="content-search"
            onSubmit={(event) => {
              event.preventDefault();
              setSearchQuery(searchText.trim());
            }}
          >
            <label htmlFor="session-content-search">
              Search session content
            </label>
            <div>
              <input
                id="session-content-search"
                value={searchText}
                onChange={(event) => setSearchText(event.target.value)}
                placeholder="Keywords across all imported agent tools"
                required
              />
              <button type="submit">Search</button>
            </div>
          </form>
          {search.isFetching && <p role="status">Searching session content…</p>}
          {search.isError && (
            <ErrorState
              message="Search is temporarily unavailable"
              retry={search.refetch}
            />
          )}
          {searchQuery && search.data?.length === 0 && (
            <p>No keyword matches</p>
          )}
          {search.data && search.data.length > 0 && (
            <section
              className="search-results"
              aria-label="Session search results"
            >
              <h2>Cross-tool matches</h2>
              <ol>
                {search.data.map((result) => (
                  <li key={result.event_id}>
                    <button
                      type="button"
                      onClick={() => setSelectedId(result.native_session_id)}
                    >
                      <span className="tool-badge">{result.tool_family}</span>
                      <strong>{result.native_session_id}</strong>
                      <span>{eventLabel(result.kind)}</span>
                      <p>{result.content}</p>
                      <small>{result.source_path}</small>
                    </button>
                  </li>
                ))}
              </ol>
            </section>
          )}
          <div className="timeline-heading">
            <div>
              <p className="eyebrow">CHRONOLOGICAL TRACE</p>
              <h2 id="timeline-heading">
                {selectedSession?.thread_title ||
                  effectiveId ||
                  "Select a session"}
              </h2>
            </div>
            <label>
              Event kind
              <select
                value={kind}
                onChange={(event) => setKind(event.target.value)}
              >
                <option value="">All events</option>
                <option value="user_message">User messages</option>
                <option value="assistant_message">Assistant messages</option>
                <option value="tool_call">Tool calls</option>
                <option value="tool_result">Tool results</option>
                <option value="error">Errors</option>
              </select>
            </label>
            <button
              type="button"
              className="reveal"
              onClick={() => setConfirmingContent(true)}
              disabled={showContent}
            >
              {showContent
                ? "Timeline content visible"
                : "Show timeline content"}
            </button>
          </div>
          {confirmingContent && (
            <div
              role="dialog"
              aria-label="Timeline content warning"
              className="warning"
            >
              <p>
                Normalized session content and source paths may contain secrets
                or private data.
              </p>
              <button
                type="button"
                onClick={() => {
                  setShowContent(true);
                  setConfirmingContent(false);
                }}
              >
                Show content now
              </button>
              <button type="button" onClick={() => setConfirmingContent(false)}>
                Cancel
              </button>
            </div>
          )}
          {events.isPending && effectiveId && (
            <p role="status">Loading events…</p>
          )}
          {events.isError && (
            <ErrorState
              message="Timeline is temporarily unavailable"
              retry={events.refetch}
            />
          )}
          {events.data?.items.length === 0 && <p>No matching events</p>}
          <ol className="timeline">
            {groupEvents(events.data?.items ?? []).map((group) => (
              <li
                key={group[0].event_id}
                aria-label={
                  group.length > 1 ? "Tool activity group" : undefined
                }
                className={group.length > 1 ? "event-group" : undefined}
              >
                {group.map((event) => (
                  <EventCard
                    key={event.event_id}
                    event={event}
                    client={client}
                    revealContent={showContent}
                  />
                ))}
              </li>
            ))}
          </ol>
        </section>
      </div>
    </main>
  );
}

function CorrelationReview({ client }: { client: SessionMeshClient }) {
  const cache = useQueryClient();
  const candidates = useQuery({
    queryKey: ["correlation-candidates"],
    queryFn: () => client.listCorrelationCandidates(),
  });
  const review = async (id: string, decision: "accept" | "reject") => {
    await client.reviewCorrelationCandidate(id, decision);
    await cache.invalidateQueries({ queryKey: ["correlation-candidates"] });
    await cache.invalidateQueries({ queryKey: ["global-sessions"] });
  };
  const pending =
    candidates.data?.filter((candidate) => candidate.status === "pending") ??
    [];
  return (
    <section
      className="correlation-review"
      aria-labelledby="correlation-review-heading"
    >
      <h2 id="correlation-review-heading">Correlation review</h2>
      {candidates.isPending && <p role="status">Loading candidates…</p>}
      {pending.length === 0 && !candidates.isPending && (
        <p>No uncertain candidates</p>
      )}
      {pending.map((candidate) => (
        <article key={candidate.id}>
          <strong>{Math.round(candidate.score * 100)}% match</strong>
          <p>{candidate.left_native_session_id}</p>
          <p>↔ {candidate.right_native_session_id}</p>
          <ul>
            {candidate.evidence.map((evidence, index) => (
              <li key={`${candidate.id}-${index}`}>
                {formatEvidence(evidence)}
              </li>
            ))}
          </ul>
          <div className="review-actions">
            <button
              type="button"
              onClick={() => void review(candidate.id, "accept")}
            >
              Link candidate
            </button>
            <button
              type="button"
              onClick={() => void review(candidate.id, "reject")}
            >
              Reject candidate
            </button>
          </div>
        </article>
      ))}
    </section>
  );
}

function formatEvidence(evidence: Record<string, unknown>): string {
  const signal = String(evidence.signal ?? "signal");
  if (signal === "workspace")
    return `Workspace match: ${String(evidence.matched)}`;
  if (signal === "temporal_gap")
    return `Time gap: ${String(evidence.seconds)} seconds`;
  if (signal === "content_jaccard") {
    return `Content similarity: ${Math.round(Number(evidence.similarity ?? 0) * 100)}%`;
  }
  return JSON.stringify(evidence);
}

function GlobalSessionReview({
  client,
  nativeSessionId,
}: {
  client: SessionMeshClient;
  nativeSessionId: string;
}) {
  const cache = useQueryClient();
  const [activeId, setActiveId] = useState("");
  const globals = useQuery({
    queryKey: ["global-sessions"],
    queryFn: () => client.listGlobalSessions(),
  });
  const effectiveGlobalId = activeId || globals.data?.[0]?.id || "";
  const detail = useQuery({
    queryKey: ["global-session", effectiveGlobalId],
    queryFn: () => client.getGlobalSession(effectiveGlobalId),
    enabled: Boolean(effectiveGlobalId),
  });
  const linked = detail.data?.members.some(
    (member) => member.native_session_id === nativeSessionId,
  );
  const refresh = async () => {
    await cache.invalidateQueries({ queryKey: ["global-sessions"] });
    await cache.invalidateQueries({
      queryKey: ["global-session", effectiveGlobalId],
    });
  };
  const create = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const objective = String(new FormData(form).get("objective") ?? "").trim();
    if (!objective) return;
    const created = await client.createGlobalSession(objective);
    setActiveId(created.id);
    form.reset();
    await refresh();
  };
  return (
    <section className="global-review" aria-labelledby="global-heading">
      <h2 id="global-heading">Global context</h2>
      <form onSubmit={(event) => void create(event)}>
        <label htmlFor="objective">New objective</label>
        <input id="objective" name="objective" required />
        <button type="submit">Create</button>
      </form>
      {globals.data && globals.data.length > 0 && (
        <label>
          Active global session
          <select
            value={effectiveGlobalId}
            onChange={(event) => setActiveId(event.target.value)}
          >
            {globals.data.map((session) => (
              <option value={session.id} key={session.id}>
                {session.objective}
              </option>
            ))}
          </select>
        </label>
      )}
      {effectiveGlobalId && nativeSessionId && (
        <div className="review-actions">
          {linked ? (
            <button
              type="button"
              onClick={() =>
                void client
                  .unlinkSession(effectiveGlobalId, nativeSessionId)
                  .then(refresh)
              }
            >
              Unlink selected session
            </button>
          ) : (
            <>
              <button
                type="button"
                onClick={() =>
                  void client
                    .linkSession(effectiveGlobalId, nativeSessionId)
                    .then(refresh)
                }
              >
                Link selected session
              </button>
              <button
                type="button"
                onClick={() =>
                  void client
                    .rejectSession(effectiveGlobalId, nativeSessionId)
                    .then(refresh)
                }
              >
                Reject correlation
              </button>
            </>
          )}
        </div>
      )}
    </section>
  );
}

function ErrorState({
  message,
  retry,
}: {
  message: string;
  retry: () => unknown;
}) {
  return (
    <div role="alert" className="error-state">
      <p>{message}</p>
      <button type="button" onClick={() => void retry()}>
        Retry
      </button>
    </div>
  );
}

function EventCard({
  event,
  client,
  revealContent,
}: {
  event: EventSummary;
  client: SessionMeshClient;
  revealContent: boolean;
}) {
  const [confirming, setConfirming] = useState(false);
  const [detail, setDetail] = useState<CanonicalEvent>();
  const reveal = async () => {
    setDetail(await client.getEvent(event.event_id));
    setConfirming(false);
  };
  useEffect(() => {
    if (!revealContent || detail) return;
    let active = true;
    void client.getEvent(event.event_id).then((loaded) => {
      if (active) setDetail(loaded);
    });
    return () => {
      active = false;
    };
  }, [client, detail, event.event_id, revealContent]);
  return (
    <article className={`event event-${event.kind || "unknown"}`}>
      <div className="event-title">
        <strong>{eventLabel(event.kind)}</strong>
        <time dateTime={event.timestamp}>
          {new Date(event.timestamp).toLocaleString()}
        </time>
      </div>
      <p className="event-meta">
        #{event.sequence} {event.branch ? `· ${event.branch}` : ""}
        {event.cwd ? ` · ${event.cwd}` : ""}
      </p>
      {!detail && (
        <button
          type="button"
          className="reveal"
          onClick={() => setConfirming(true)}
        >
          Reveal event data
        </button>
      )}
      {confirming && (
        <div
          role="dialog"
          aria-label="Sensitive event data"
          className="warning"
        >
          <p>
            Native event data may contain secrets, prompts, or file contents.
          </p>
          <button type="button" onClick={() => void reveal()}>
            Reveal now
          </button>
          <button type="button" onClick={() => setConfirming(false)}>
            Cancel
          </button>
        </div>
      )}
      {detail && <EventContent detail={detail} />}
    </article>
  );
}

function EventContent({ detail }: { detail: CanonicalEvent }) {
  const preferredFields = [
    "text",
    "command",
    "stdout",
    "output",
    "content",
    "message",
  ];
  const values = preferredFields
    .map((field) => [field, detail.payload[field]] as const)
    .filter((entry) => entry[1] !== undefined && entry[1] !== null);
  const sourcePath = String(detail.provenance.source_path ?? "Unknown source");
  return (
    <section className="event-content" aria-label="Event content">
      {values.length > 0 ? (
        values.map(([field, value]) => (
          <div key={field}>
            <strong>{eventLabel(field)}</strong>
            <pre tabIndex={0}>{formatContent(value)}</pre>
          </div>
        ))
      ) : (
        <pre tabIndex={0}>{JSON.stringify(detail.payload, null, 2)}</pre>
      )}
      <details>
        <summary>Full canonical event</summary>
        <pre tabIndex={0}>{JSON.stringify(detail, null, 2)}</pre>
      </details>
      <p className="event-source">
        Source: <span>{sourcePath}</span>
      </p>
    </section>
  );
}

function formatContent(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

function groupEvents(events: EventSummary[]): EventSummary[][] {
  const groups: EventSummary[][] = [];
  for (const event of events) {
    const previous = groups.at(-1);
    if (
      event.kind === "tool_result" &&
      previous?.at(-1)?.kind === "tool_call"
    ) {
      previous.push(event);
    } else {
      groups.push([event]);
    }
  }
  return groups;
}
