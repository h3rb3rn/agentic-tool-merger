/** One imported native session as `sessionmesh_list_sessions` reports it. */
export type SessionRow = {
  id: string;
  tool: string;
  title: string;
  lastActivity: string | null;
  events: number;
};

/** One session that contains search matches, grouped from the event matches. */
export type SearchHit = {
  id: string;
  tool: string;
  title: string;
  /** How many matching events the session holds within the searched window. */
  matches: number;
  /** Timestamp of the session's newest matching event. */
  latest: string | null;
};

declare module "claude-code" {
  interface PluginState {
    "sessionmesh-pane": {
      sessions: Shaped<SessionRow[]>;
      /** `all` or one `tool_family` such as `opencode` or `claude-code`. */
      filter: string;
      /** What the last refresh or search did: a short line for the pane footer. */
      status: string;
      /** The submitted search text; empty while the pane shows the session list. */
      query: string;
      hits: Shaped<SearchHit[]>;
    };
  }
}
