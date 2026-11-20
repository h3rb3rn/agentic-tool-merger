import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Faq } from "./Faq";

afterEach(() => {
  vi.unstubAllGlobals();
  Reflect.deleteProperty(navigator, "clipboard");
});

describe("session handoff FAQ", () => {
  it("covers native resume and cross-client context for every supported client", () => {
    render(<Faq />);
    for (const name of [
      "Codex",
      "Claude Code",
      "Agy",
      "OpenCode",
      "Grok Build",
      "Continue CLI / IDE",
    ]) {
      expect(screen.getByRole("heading", { name })).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: `Copy ${name} handoff command` }),
      ).toBeInTheDocument();
    }
    expect(
      screen.getByText("opencode --session NATIVE_SESSION_ID", { exact: true }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("grok --resume NATIVE_SESSION_ID", { exact: true }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("cn --resume", { exact: true }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "What do I do after a rate limit?" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Cross-client continuation starts a new native session/),
    ).toBeInTheDocument();
    expect(screen.getByText(/Agy prompt history/)).toBeInTheDocument();
  });

  it("selects the command and gives manual copy guidance when clipboard access is unavailable", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: undefined,
    });
    render(<Faq />);
    fireEvent.click(
      screen.getByRole("button", { name: "Copy OpenCode handoff command" }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(/Ctrl\+C/);
    expect(window.getSelection()?.toString()).toContain("opencode --prompt");
    expect(window.getSelection()?.toString()).toContain(
      "sessionmesh_get_handoff",
    );
  });
  it("reports success only after clipboard access succeeds", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<Faq />);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Copy Grok Build known native session",
      }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("Copied.");
    expect(writeText).toHaveBeenCalledWith("grok --resume NATIVE_SESSION_ID");
  });

  it("offers manual copy when clipboard permission is denied", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn().mockRejectedValue(new Error("Permission denied")),
      },
    });
    render(<Faq />);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Copy OpenCode known native session",
      }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("Ctrl+C");
    expect(window.getSelection()?.toString()).toBe(
      "opencode --session NATIVE_SESSION_ID",
    );
  });
});
