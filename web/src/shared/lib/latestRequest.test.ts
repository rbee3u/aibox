import { describe, expect, it } from "vitest";
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
});
