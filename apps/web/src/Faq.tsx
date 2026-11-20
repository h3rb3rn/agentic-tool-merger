import { useRef, useState } from "react";

const ROOT = "/opt/deployment/agenttool-merger";
const HANDOFF_PROMPT =
  "Call sessionmesh_get_current and sessionmesh_get_handoff. Verify the source sessions, workspace, repository state and open tasks, then continue the next open task.";

interface AgentGuide {
  id: string;
  name: string;
  version: string;
  choose?: string;
  latest: string;
  byId?: string;
  launch: string;
  inspect: string;
  note: string;
}

/** Native commands are verified against installed CLI help, independently of imported IDs. */
const AGENTS: AgentGuide[] = [
  {
    id: "codex",
    name: "Codex",
    version: "0.170.0",
    choose: "codex resume",
    latest: "codex resume --last",
    byId: "codex resume NATIVE_SESSION_ID",
    launch: "codex",
    inspect: "codex mcp get sessionmesh",
    note: "Use codex resume --all to include other working directories in the native picker. The configured SessionMesh startup instruction requests shared context; verify the handoff when resuming an older conversation.",
  },
  {
    id: "claude",
    name: "Claude Code",
    version: "2.1.328",
    choose: "claude --resume",
    latest: "claude --continue",
    byId: "claude --resume NATIVE_SESSION_ID",
    launch: "claude",
    inspect: "claude mcp get sessionmesh",
    note: "The connector installs a SessionStart hook for automatic handoff delivery. CLI, editor and desktop histories can be separate; choose the source conversation in its original surface.",
  },
  {
    id: "agy",
    name: "Agy",
    version: "1.5.13",
    latest: "agy --continue",
    byId: "agy --conversation NATIVE_CONVERSATION_ID",
    launch: "agy --prompt-interactive",
    inspect: "agy mcp list",
    note: "Use a conversation ID from Agy's own history. SessionMesh imports Agy prompt history; full Agy assistant trajectories are not currently imported. Check the source conversation for any missing response before relying on the shared handoff.",
  },
  {
    id: "opencode",
    name: "OpenCode",
    version: "2.2.7",
    choose: "opencode session list",
    latest: "opencode --continue",
    byId: "opencode --session NATIVE_SESSION_ID",
    launch: "opencode --prompt",
    inspect: "opencode mcp list",
    note: "Use an existing ID from opencode session list: --session can create a session when the ID does not exist. If opencode is outside PATH, use ~/.opencode/bin/opencode. These commands target OpenCode v2.",
  },
  {
    id: "grok",
    name: "Grok Build",
    version: "1.0.89",
    choose: "grok sessions list",
    latest: "grok --continue",
    byId: "grok --resume NATIVE_SESSION_ID",
    launch: "grok",
    inspect: "grok mcp doctor sessionmesh",
    note: "Grok also accepts a saved session title with --resume. SessionMesh reads its authoritative conversation updates, including prompts, replies and reasoning, without changing Grok's native history.",
  },
  {
    id: "continue",
    name: "Continue CLI / IDE",
    version: "cn 1.5.43",
    choose: "cn ls",
    latest: "cn --resume",
    launch: "cn",
    inspect: `${ROOT}/scripts/check-sessionmesh-mcp`,
    note: "cn ls is the native CLI session picker. The verified --resume flag resumes the latest session and takes no session-ID argument. For Continue IDE, use its own history UI and configured MCP tools. CLI and IDE histories can differ. The check below verifies SessionMesh transport; confirm its tools are also available inside Continue.",
  },
];

/** Copies public instructions or selects them for manual copy on HTTP deployments. */
function CopyBlock({
  label,
  text,
  kind = "Terminal command",
}: {
  label: string;
  text: string;
  kind?: "Terminal command" | "Agent prompt";
}) {
  const code = useRef<HTMLElement>(null);
  const [status, setStatus] = useState("");
  const copy = async () => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
        setStatus("Copied.");
        return;
      }
    } catch {
      // Clipboard permission and secure-context requirements vary by browser.
    }
    const selection = window.getSelection();
    if (selection && code.current) {
      const range = document.createRange();
      range.selectNodeContents(code.current);
      selection.removeAllRanges();
      selection.addRange(range);
      setStatus("Text selected. Press Ctrl+C or ⌘C to copy.");
    } else {
      setStatus("Select the text and press Ctrl+C or ⌘C to copy.");
    }
  };
  return (
    <div className="faq-command">
      <div className="faq-command-heading">
        <span>
          {label} · {kind}
        </span>
        <button
          type="button"
          aria-label={`Copy ${label}`}
          onClick={() => void copy()}
        >
          Copy
        </button>
      </div>
      <pre tabIndex={0}>
        <code ref={code}>{text}</code>
      </pre>
      {status && (
        <p role="status" className="faq-copy-status">
          {status}
        </p>
      )}
    </div>
  );
}

/** Public, static help: it never executes commands or requests private session data. */
export function Faq() {
  return (
    <main className="faq-shell">
      <header className="faq-header">
        <div>
          <p className="eyebrow">SESSIONMESH HELP</p>
          <h1>FAQ: sessions and agent handoffs</h1>
        </div>
        <a href="#sessions">Back to sessions</a>
      </header>
      <p>
        Use the original client to reopen its history, or carry shared project
        context into another agent.
      </p>
      <nav aria-label="FAQ topics" className="faq-topics">
        <a href="#faq-rate-limit">After a rate limit</a>
        <a href="#faq-context">Retrieve context</a>
        {AGENTS.map((agent) => (
          <a key={agent.id} href={`#faq-agent-${agent.id}`}>
            {agent.name}
          </a>
        ))}
        <a href="#faq-troubleshooting">Connection help</a>
      </nav>

      <section className="faq-section" id="faq-basics">
        <h2>Can another client resume the same native session?</h2>
        <p>
          Native history belongs to its client. Cross-client continuation starts
          a new native session with a shared SessionMesh handoff. Each client
          keeps its own transcript and native session ID.
        </p>
        <p>
          Run commands on your host from the same project checkout or worktree.
          Replace <code>/path/to/project</code> and the native-ID placeholders
          before running a command. Get native IDs from the original client;
          SessionMesh global IDs such as <code>gs_…</code> and namespaced
          imported IDs are not native resume arguments.
        </p>
      </section>

      <section className="faq-section" id="faq-rate-limit">
        <h2>What do I do after a rate limit?</h2>
        <ol>
          <li>
            Keep the project checkout and worktree. Note the branch, unfinished
            task, pending tests and any uncommitted changes.
          </li>
          <li>
            Check that the source conversation has appeared in SessionMesh.
            Review uncertain links in Correlation review and confirm the
            sessions belong to the same task.
          </li>
          <li>
            Choose another client or model with available quota. Use its handoff
            command below from the same project directory.
          </li>
          <li>
            Have the new agent inspect <code>sessionmesh_get_current</code> and{" "}
            <code>sessionmesh_get_handoff</code>, confirm the expected source
            sessions and verify the files before continuing.
          </li>
          <li>
            To return later, resume the original client's native session and
            request an updated SessionMesh handoff so it can see work done by
            the other client.
          </li>
        </ol>
        <p>
          The source agent does not need to answer another prompt once its
          conversation is already imported. SessionMesh transfers context and
          preserves provenance; provider quotas and account access remain
          controlled by each provider.
        </p>
        <p>
          <strong>Example:</strong> after Codex reaches a limit, use the Claude
          Code handoff command. After work in Claude, return with{" "}
          <code>codex resume --last</code> and ask Codex to retrieve the latest
          shared context before editing.
        </p>
      </section>

      <section className="faq-section" id="faq-context">
        <h2>How do I retrieve context from another agent?</h2>
        <p>
          The client's working directory and SessionMesh project scope determine
          the current handoff. The agent selector in the Web UI browses
          conversations. Use <code>sessionmesh_get_current</code> to check which
          native sessions actually belong to the current global session,
          especially when several tasks share a repository.
        </p>
        <CopyBlock
          label="Current project context"
          kind="Agent prompt"
          text={HANDOFF_PROMPT}
        />
        <CopyBlock
          label="Search previous conversations"
          kind="Agent prompt"
          text={
            'Call sessionmesh_search with {"query":"unique task keywords","limit":20}. Identify the relevant native_session_id and source provenance, and check that they match this project and task.'
          }
        />
        <p>
          Send these prompts inside your agent. MCP tool names are not terminal
          commands. Search returns event references and provenance; read the
          matching conversation through the Web UI's Search session content and
          Show timeline content controls, or an authorized read of the original
          source.
        </p>
        <p>
          The read tools work with the default connector permissions. Recording
          tasks or decisions requires separately authorized MCP writes.
        </p>
        <CopyBlock
          label="Verify the SessionMesh connection"
          text={`cd /path/to/project\n${ROOT}/scripts/check-sessionmesh-mcp`}
        />
        <p>
          For a manual handoff outside a client, run the command below from the
          project directory and provide the resulting historical context to the
          destination agent for verification. Its output contains private
          project context.
        </p>
        <CopyBlock
          label="Read a manual project handoff"
          text={`cd /path/to/project\n${ROOT}/scripts/sessionmesh-session-start`}
        />
      </section>

      <section className="faq-section" id="faq-agents">
        <h2>Commands for each agent</h2>
        <p>
          The handoff commands require a running SessionMesh service and the
          destination client's MCP connector. Copying a command only copies or
          selects text; run it yourself in the intended project.
        </p>
        <div className="faq-agent-grid">
          {AGENTS.map((agent) => (
            <article
              className="faq-agent"
              id={`faq-agent-${agent.id}`}
              key={agent.id}
            >
              <h3>{agent.name}</h3>
              <p className="faq-version">Checked with {agent.version}</p>
              {agent.choose && (
                <CopyBlock
                  label={`${agent.name} native session picker or list`}
                  text={agent.choose}
                />
              )}
              <CopyBlock
                label={`${agent.name} latest native session`}
                text={agent.latest}
              />
              {agent.byId && (
                <CopyBlock
                  label={`${agent.name} known native session`}
                  text={agent.byId}
                />
              )}
              <CopyBlock
                label={`${agent.name} handoff command`}
                text={`cd /path/to/project\n${agent.launch} '${HANDOFF_PROMPT}'`}
              />
              <CopyBlock
                label={`${agent.name} MCP setup check`}
                text={agent.inspect}
              />
              <p>{agent.note}</p>
            </article>
          ))}
        </div>
      </section>

      <section className="faq-section" id="faq-troubleshooting">
        <h2>Why is context missing or the connection closed?</h2>
        <details>
          <summary>How do I configure a client connector?</summary>
          <p>
            Run the installer on the host where your clients run, then restart
            the selected clients. Commands here use this deployment's
            installation path; adjust it for another installation.
          </p>
          <CopyBlock
            label="Install the main CLI connectors"
            text={`${ROOT}/scripts/install-sessionmesh-connectors codex claude agy opencode grok`}
          />
          <p>
            Continue is optional. Its installer requires an existing Continue
            configuration and jq-style Python <code>yq</code>.
          </p>
          <CopyBlock
            label="Install the optional Continue connector"
            text={`${ROOT}/scripts/install-sessionmesh-connectors continue`}
          />
        </details>
        <details>
          <summary>
            What does Transport closed mean after a container restart?
          </summary>
          <p>
            Restart the affected client from the project directory to create a
            fresh MCP process. Run the connection check above. An already
            running client's old stdio process can have ended when the container
            restarted.
          </p>
        </details>
        <details>
          <summary>
            Why does a handoff omit recent messages or choose a different task?
          </summary>
          <p>
            Check the source conversation in the Web UI, allow initial import to
            catch up and verify the working directory, project marker and
            global-session memberships. Unsupported source formats and malformed
            native records can block import. Agy assistant trajectories are
            outside current coverage. The native source remains available in its
            original client.
          </p>
          <p>
            Use task-specific search terms and check provenance before
            continuing. A compact handoff is a bounded context window; original
            imported prompts and AI replies remain available separately.
          </p>
        </details>
        <p className="faq-version">
          Command syntax checked against installed CLI help on 2026-11-20. Use
          the native client's help after upgrades.
        </p>
      </section>
    </main>
  );
}
