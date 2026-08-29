import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { usePolling } from "@/shared/hooks/usePolling";

describe("usePolling", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("uses updated callbacks without cancelling or restarting an in-flight poll", async () => {
    let finishFirst: (() => void) | undefined;
    const firstRun = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishFirst = resolve;
        }),
    );
    const nextRun = vi.fn().mockResolvedValue(undefined);
    const firstCancel = vi.fn();
    const nextCancel = vi.fn();
    const { rerender, unmount } = renderHook(
      ({ run, onCancel }) => usePolling({ enabled: true, intervalMs: 5000, run, onCancel }),
      { initialProps: { run: firstRun, onCancel: firstCancel } },
    );

    expect(firstRun).toHaveBeenCalledOnce();
    rerender({ run: nextRun, onCancel: nextCancel });
    expect(firstCancel).not.toHaveBeenCalled();
    expect(nextCancel).not.toHaveBeenCalled();
    expect(nextRun).not.toHaveBeenCalled();

    await act(async () => {
      finishFirst?.();
      await Promise.resolve();
    });
    await act(() => vi.advanceTimersByTimeAsync(5000));
    expect(nextRun).toHaveBeenCalledWith(false);

    unmount();
    expect(nextCancel).toHaveBeenCalledOnce();
  });
});
