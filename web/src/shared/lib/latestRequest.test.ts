import { describe, expect, it, vi } from "vitest";
import { LatestRequest } from "@/shared/lib/latestRequest";

describe("LatestRequest", () => {
  it("aborts the previous lease and keeps ownership with the newest one", () => {
    const owner = new LatestRequest();
    const first = owner.begin();
    const second = owner.begin();

    expect(first.signal.aborted).toBe(true);
    expect(first.isCurrent()).toBe(false);
    expect(second.isCurrent()).toBe(true);
  });

  it("does not release a newer request when an older request finishes", () => {
    const owner = new LatestRequest();
    const first = owner.begin();
    const second = owner.begin();

    first.release();
    expect(second.isCurrent()).toBe(true);
    second.release();
    expect(second.isCurrent()).toBe(false);
  });

  it("cancels and clears the current request", () => {
    const owner = new LatestRequest();
    const request = owner.begin();

    owner.cancel();

    expect(request.signal.aborted).toBe(true);
    expect(request.isCurrent()).toBe(false);
  });

  it("commits only the newest response when an older load ignores cancellation", async () => {
    const owner = new LatestRequest();
    let completeFirst!: (value: string) => void;
    const loaded = vi.fn();
    const settled = vi.fn();
    const callbacks = { loaded, settled, failed: vi.fn() };
    const first = owner.run(
      () =>
        new Promise<string>((resolve) => {
          completeFirst = resolve;
        }),
      callbacks,
    );

    expect(await owner.run(() => Promise.resolve("new"), callbacks)).toBe("new");
    completeFirst("old");
    expect(await first).toBeNull();
    expect(loaded.mock.calls).toEqual([["new"]]);
    expect(settled).toHaveBeenCalledTimes(1);
  });

  it("ignores stale failures and does not settle a newer request", async () => {
    const owner = new LatestRequest();
    let failFirst!: (cause: Error) => void;
    const callbacks = { loaded: vi.fn(), failed: vi.fn(), settled: vi.fn() };
    const first = owner.run(
      () =>
        new Promise<string>((_, reject) => {
          failFirst = reject;
        }),
      callbacks,
    );
    const newer = owner.begin();

    failFirst(new Error("old failure"));
    expect(await first).toBeNull();
    expect(callbacks.failed).not.toHaveBeenCalled();
    expect(callbacks.settled).not.toHaveBeenCalled();
    expect(newer.isCurrent()).toBe(true);
  });

  it("reports a current failure once and releases its lease", async () => {
    const owner = new LatestRequest();
    const cause = new Error("failed");
    const callbacks = { loaded: vi.fn(), failed: vi.fn(), settled: vi.fn() };
    expect(await owner.run(() => Promise.reject(cause), callbacks)).toBeNull();
    expect(callbacks.failed).toHaveBeenCalledWith(cause);
    expect(callbacks.settled).toHaveBeenCalledOnce();
    expect(callbacks.loaded).not.toHaveBeenCalled();
  });

  it("preserves a replacement lease started by a successful response callback", async () => {
    const owner = new LatestRequest();
    const replacements: Array<ReturnType<LatestRequest["begin"]>> = [];
    const settled = vi.fn();
    await owner.run(() => Promise.resolve("loaded"), {
      loaded: () => {
        replacements.push(owner.begin());
      },
      failed: vi.fn(),
      settled,
    });
    expect(replacements).toHaveLength(1);
    expect(settled).not.toHaveBeenCalled();
    expect(replacements[0].isCurrent()).toBe(true);
  });
});
