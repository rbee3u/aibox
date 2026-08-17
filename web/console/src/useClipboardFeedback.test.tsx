import { act, renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { deferred } from "./test/fixtures";
import { useClipboardFeedback } from "./useClipboardFeedback";

it.each(["resolve", "reject"] as const)(
  "keeps feedback from the newest clipboard request when an older write settles with %s",
  async (olderOutcome) => {
    vi.useFakeTimers();
    const first = deferred<void>();
    const second = deferred<void>();
    const writeText = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const { result } = renderHook(() => useClipboardFeedback<string>());

    let firstCopy!: Promise<void>;
    let secondCopy!: Promise<void>;
    act(() => {
      firstCopy = result.current[1]("first", "first");
      secondCopy = result.current[1]("second", "second");
    });

    await act(async () => {
      second.resolve();
      await secondCopy;
    });
    expect(result.current[0]).toBe("second");

    await act(async () => {
      if (olderOutcome === "resolve") first.resolve();
      else first.reject(new Error("older clipboard request failed"));
      await firstCopy;
    });
    expect(result.current[0]).toBe("second");
    expect(vi.getTimerCount()).toBe(1);

    await act(() => vi.runOnlyPendingTimersAsync());
    expect(result.current[0]).toBeNull();
  },
);

it("does not schedule feedback after the consumer unmounts", async () => {
  vi.useFakeTimers();
  const pending = deferred<void>();
  vi.stubGlobal("navigator", {
    clipboard: { writeText: vi.fn().mockReturnValue(pending.promise) },
  });
  const { result, unmount } = renderHook(() => useClipboardFeedback());

  let copy!: Promise<void>;
  act(() => {
    copy = result.current[1]("value", true);
  });
  unmount();
  await act(async () => {
    pending.resolve();
    await copy;
  });

  expect(vi.getTimerCount()).toBe(0);
});

it("clears completed feedback when a newer clipboard request starts and fails", async () => {
  vi.useFakeTimers();
  const pending = deferred<void>();
  const writeText = vi.fn().mockResolvedValueOnce(undefined).mockReturnValueOnce(pending.promise);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const { result } = renderHook(() => useClipboardFeedback<string>());

  await act(async () => {
    await result.current[1]("first", "first");
  });
  expect(result.current[0]).toBe("first");
  expect(vi.getTimerCount()).toBe(1);

  let secondCopy!: Promise<void>;
  act(() => {
    secondCopy = result.current[1]("second", "second");
  });
  expect(result.current[0]).toBeNull();
  expect(vi.getTimerCount()).toBe(0);

  await act(async () => {
    pending.reject(new Error("clipboard unavailable"));
    await secondCopy;
  });
  expect(result.current[0]).toBeNull();
  expect(vi.getTimerCount()).toBe(0);
});
