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

describe("authentication errors", () => {
  it("preserves HTTP status and requests reauthentication only for 401", async () => {
    const unauthorized = vi.fn();
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValueOnce(
          new Response('{"code":"unauthorized"}', { status: 401 }),
        )
        .mockResolvedValueOnce(new Response("{}", { status: 500 })),
    );
    const api = createApiClient("old-token", unauthorized);
    await expect(api.listSessions()).rejects.toMatchObject({ status: 401 });
    expect(unauthorized).toHaveBeenCalledTimes(1);
    await expect(api.listSessions()).rejects.toMatchObject({ status: 500 });
    expect(unauthorized).toHaveBeenCalledTimes(1);
  });
});

it("returns a rejected stream to authentication without a reconnect loop", async () => {
  const unauthorized = vi.fn();
  const fetch = vi.fn().mockResolvedValue(new Response("{}", { status: 401 }));
  vi.stubGlobal("fetch", fetch);
  const stop = createApiClient("old-token", unauthorized).subscribe(
    undefined,
    vi.fn(),
  );
  await waitFor(() => expect(unauthorized).toHaveBeenCalledTimes(1));
  expect(fetch).toHaveBeenCalledTimes(1);
  stop();
});
