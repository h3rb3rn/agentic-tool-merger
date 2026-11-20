import { expect, test } from "claude-code/testing";

const PANE = {
  plugin: "sessionmesh-pane",
  component: "Pane",
  requestId: "sessionmesh",
  props: {
    title: "SessionMesh",
    isFocused: false,
    bodyColumns: 80,
    placement: "dock",
    scroll: { offset: 0, bodyRows: 24 },
    view: {},
  },
} as const;

const result = (structuredContent: unknown) => ({
  value: { content: [], isError: false, structuredContent },
});

test("a search quotes every word and groups matches by session", async ($, on) => {
  const searches: string[] = [];
  on("mcp.call", (_, e) => {
    if (e.tool === "sessionmesh_search") {
      searches.push(String(e.args.query));
      return result({
        items: [
          { native_session_id: "codex-1", timestamp: "2026-11-01T10:00:00Z" },
          { native_session_id: "codex-1", timestamp: "2026-11-02T10:00:00Z" },
          { native_session_id: "grok-1", timestamp: "2026-11-03T10:00:00Z" },
        ],
        next_cursor: null,
      });
    }
    if (e.tool === "sessionmesh_get_session") {
      const id = String(e.args.native_session_id);
      return result({
        session: { tool_family: id.split("-")[0], title: `Title of ${id}` },
      });
    }
    return result({ items: [] });
  });

  const ui = await $.ui.mount({ ...PANE, surface: "terminal" });
  await ui.input({ key: "search", text: 'claude-code "x"' });

  expect(searches).toEqual(['"claude-code" """x"""']);
  const first = await ui.find({ type: "Text", text: /Title of codex-1/ });
  expect(first).toBeTruthy();
  const second = await ui.find({ type: "Text", text: /Title of grok-1/ });
  expect(second).toBeTruthy();
  const counts = await ui.find({ type: "Text", text: /2 hits/ });
  expect(counts).toBeTruthy();
  await ui.unmount();
});

test("a failing search is shown instead of an empty result", async ($, on) => {
  on("mcp.call", () => ({
    value: {
      content: [{ type: "text", text: "storage operation failed" }],
      isError: true,
    },
  }));

  const ui = await $.ui.mount({ ...PANE, surface: "terminal" });
  await ui.input({ key: "search", text: "anything" });

  const footer = await ui.find({ type: "Text", text: /Search failed/ });
  expect(footer?.text).toContain("storage operation failed");
  await ui.unmount();
});
