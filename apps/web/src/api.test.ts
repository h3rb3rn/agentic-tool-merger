import { afterEach, describe, expect, it, vi } from "vitest";
import { waitFor } from "@testing-library/react";

import { createApiClient } from "./api";

afterEach(() => vi.unstubAllGlobals());

describe("event detail loading", () => {
  it("limits concurrent requests and releases capacity after a failure", async () => {
    const pending: Array<{
      resolve: (value: Response) => void;
      reject: (reason: Error) => void;
    }> = [];
    const fetch = vi.fn(
      () =>
        new Promise<Response>((resolve, reject) =>
          pending.push({ resolve, reject }),
        ),
    );
    vi.stubGlobal("fetch", fetch);
    const api = createApiClient("test-token");
    const results = Array.from({ length: 12 }, (_, index) =>
      api.getEvent(`event-${index}`).catch(() => undefined),
    );
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(4));
    pending[0].reject(new Error("offline"));
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(5));
    for (let index = 1; index < 12; index++) {
      await waitFor(() => expect(pending.length).toBeGreaterThan(index));
      pending[index].resolve(
        new Response(JSON.stringify({ event_id: `event-${index}` })),
      );
    }
    expect((await Promise.all(results)).filter(Boolean)).toHaveLength(11);
  });

  it("does not fetch content cancelled before its queue slot becomes available", async () => {
    const pending: Array<(value: Response) => void> = [];
    const fetch = vi.fn(
      () => new Promise<Response>((resolve) => pending.push(resolve)),
    );
    vi.stubGlobal("fetch", fetch);
    const api = createApiClient("test-token");
    const active = Array.from({ length: 4 }, (_, index) =>
      api.getEvent(`event-${index}`),
    );
    const controller = new AbortController();
    const cancelled = api
      .getEvent("cancelled", controller.signal)
      .catch((error: Error) => error.name);
    controller.abort();
    pending.forEach((resolve) => resolve(new Response("{}")));
    await Promise.all(active);
    expect(await cancelled).toBe("AbortError");
    expect(fetch).toHaveBeenCalledTimes(4);
  });
});
