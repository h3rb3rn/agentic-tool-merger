/** One imported native session as `sessionmesh_list_sessions` reports it. */
export type SessionRow = {
  id: string;
  tool: string;
  title: string;
  lastActivity: string | null;
  events: number;
};

declare module "claude-code" {
  interface PluginState {
    "sessionmesh-pane": {
      sessions: Shaped<SessionRow[]>;
      /** `all` or one `tool_family` such as `opencode` or `claude-code`. */
      filter: string;
      /** What the last refresh did: a short line, shown in the pane footer. */
      status: string;
    };
  }
}
