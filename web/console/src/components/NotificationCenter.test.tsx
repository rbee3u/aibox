import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NotificationCenter, type NotificationItemData } from "./NotificationCenter";

const error: NotificationItemData = {
  id: 1,
  source: "list",
  tone: "error",
  title: "Couldn’t load request records",
  message: "scan failed",
  actionLabel: "Retry",
};

afterEach(() => vi.useRealTimers());

describe("NotificationCenter", () => {
  it("announces errors, invokes actions, and dismisses after eight visible seconds", async () => {
    vi.useFakeTimers();
    const onAction = vi.fn();
    const onDismiss = vi.fn();
    render(
      <NotificationCenter
        notifications={[error]}
        paused={false}
        onAction={onAction}
        onDismiss={onDismiss}
      />,
    );

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(error.title);
    expect(alert).toHaveTextContent(error.message);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onAction).toHaveBeenCalledWith(error);

    await act(() => vi.advanceTimersByTimeAsync(7999));
    expect(onDismiss).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(1));
    expect(onDismiss).toHaveBeenCalledWith("list");
  });

  it("pauses each timer for hover, focus, modal state, window blur, and hidden pages", async () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    const { rerender } = render(
      <NotificationCenter
        notifications={[error]}
        paused={false}
        onAction={vi.fn()}
        onDismiss={onDismiss}
      />,
    );
    const alert = screen.getByRole("alert");

    await act(() => vi.advanceTimersByTimeAsync(1000));
    fireEvent.mouseEnter(alert);
    await act(() => vi.advanceTimersByTimeAsync(8000));
    fireEvent.mouseLeave(alert);

    const retry = screen.getByRole("button", { name: "Retry" });
    fireEvent.focus(retry);
    await act(() => vi.advanceTimersByTimeAsync(8000));
    fireEvent.blur(retry);

    rerender(
      <NotificationCenter
        notifications={[error]}
        paused
        onAction={vi.fn()}
        onDismiss={onDismiss}
      />,
    );
    await act(() => vi.advanceTimersByTimeAsync(8000));
    rerender(
      <NotificationCenter
        notifications={[error]}
        paused={false}
        onAction={vi.fn()}
        onDismiss={onDismiss}
      />,
    );

    fireEvent.blur(window);
    await act(() => vi.advanceTimersByTimeAsync(8000));
    fireEvent.focus(window);

    const visibility = Object.getOwnPropertyDescriptor(document, "visibilityState");
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
    fireEvent(document, new Event("visibilitychange"));
    await act(() => vi.advanceTimersByTimeAsync(8000));
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "visible",
    });
    fireEvent(document, new Event("visibilitychange"));
    if (visibility) Object.defineProperty(document, "visibilityState", visibility);
    else Reflect.deleteProperty(document, "visibilityState");

    expect(onDismiss).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(6999));
    expect(onDismiss).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(1));
    expect(onDismiss).toHaveBeenCalledWith("list");
  });

  it("resets the timer when a source receives a different notification", async () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    const { rerender } = render(
      <NotificationCenter
        notifications={[error]}
        paused={false}
        onAction={vi.fn()}
        onDismiss={onDismiss}
      />,
    );
    await act(() => vi.advanceTimersByTimeAsync(7000));
    rerender(
      <NotificationCenter
        notifications={[{ ...error, id: 2, message: "different failure" }]}
        paused={false}
        onAction={vi.fn()}
        onDismiss={onDismiss}
      />,
    );

    await act(() => vi.advanceTimersByTimeAsync(7999));
    expect(onDismiss).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(1));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
