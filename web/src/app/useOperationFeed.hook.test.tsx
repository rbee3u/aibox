import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Operation, OperationApi } from "@/api/operations";
import { useOperationFeed } from "@/app/useOperationFeed";

const DISMISSED_KEY = "aibox-console-operation-dismissed";

function operation(overrides: Partial<Operation> = {}): Operation {
  return {
    id: "operation-1",
    kind: "install go@1.27.3",
    state: "succeeded",
    started_at: "2026-10-23T11:10:19Z",
    ended_at: "2026-10-23T11:10:32Z",
    result: "Installed go@1.27.3",
    first_sequence: 0,
    next_sequence: 1,
    logs: [{ sequence: 0, message: "Installing go@1.27.3" }],
    ...overrides,
  };
}

function feed() {
  let handlers: Parameters<OperationApi["subscribe"]>[0] | null = null;
  const api: OperationApi = {
    current: vi.fn().mockResolvedValue(null),
    cancel: vi.fn().mockResolvedValue(undefined),
    subscribe: (next) => {
      handlers = next;
      return () => {
        handlers = null;
      };
    },
  };
  return {
    api,
    emit(next: Operation | null, gap = false) {
      act(() => handlers?.onOperation(next, gap));
    },
  };
}

afterEach(() => window.localStorage.clear());

describe("useOperationFeed", () => {
  it("keeps a dismissed finished Operation hidden after the Console reloads", () => {
    const finished = operation();
    const before = feed();
    const first = renderHook(() => useOperationFeed(before.api));
    before.emit(finished);
    expect(first.result.current.visible).toEqual(finished);

    act(() => first.result.current.dismiss());
    expect(first.result.current.visible).toBeNull();
    expect(window.localStorage.getItem(DISMISSED_KEY)).toBe(finished.id);
    first.unmount();

    // A reload loses in-memory state and asks the Service for the latest
    // Operation again; the acknowledgement must survive that round trip.
    const after = feed();
    const reloaded = renderHook(() => useOperationFeed(after.api));
    after.emit(finished);
    expect(reloaded.result.current.visible).toBeNull();
    act(() => reloaded.result.current.adopt(finished));
    expect(reloaded.result.current.visible).toBeNull();
  });

  it("shows a newer Operation after an earlier result was dismissed", () => {
    window.localStorage.setItem(DISMISSED_KEY, "operation-1");
    const next = operation({
      id: "operation-2",
      kind: "install node@24.9.0",
      result: "Installed node@24.9.0",
    });
    const current = feed();
    const view = renderHook(() => useOperationFeed(current.api));

    current.emit(next);
    expect(view.result.current.visible?.id).toBe("operation-2");
  });

  it("never hides a running Operation behind a stale dismissal", () => {
    const running = operation({
      state: "running",
      ended_at: null,
      result: null,
    });
    window.localStorage.setItem(DISMISSED_KEY, running.id);
    const current = feed();
    const view = renderHook(() => useOperationFeed(current.api));

    act(() => view.result.current.adopt(running));
    expect(view.result.current.visible).toEqual(running);
  });
});
