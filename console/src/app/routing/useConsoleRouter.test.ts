import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useConsoleRouter } from "@/app/routing/useConsoleRouter";

const originalLocation = `${window.location.pathname}${window.location.search}${window.location.hash}`;

afterEach(() => {
  window.history.replaceState(null, "", originalLocation);
  vi.restoreAllMocks();
});

describe("useConsoleRouter", () => {
  it("tracks clean browser back/forward navigation", () => {
    window.history.replaceState(null, "", "/_aibox/ui/overview");
    const { result } = renderHook(() => useConsoleRouter());

    act(() => {
      window.history.pushState(null, "", "/_aibox/ui/tenants?tenant=managed%3Awork");
      window.dispatchEvent(new PopStateEvent("popstate"));
    });

    expect(result.current.route).toEqual({
      module: "tenants",
      search: "?tenant=managed%3Awork",
    });
  });

  it("holds dirty history navigation as pending and restores the location on cancel", () => {
    window.history.replaceState(null, "", "/_aibox/ui/configs?current=1");
    const { result } = renderHook(() => useConsoleRouter());
    const confirm = vi.spyOn(window, "confirm");
    act(() => result.current.recordDirty(true));

    act(() => {
      window.history.pushState(null, "", "/_aibox/ui/sessions");
      window.dispatchEvent(new PopStateEvent("popstate"));
    });

    expect(confirm).not.toHaveBeenCalled();
    expect(window.location.pathname).toBe("/_aibox/ui/configs");
    expect(result.current.route.module).toBe("configs");
    expect(result.current.pendingNavigation).toBe("/_aibox/ui/sessions");

    act(() => result.current.cancelPendingNavigation());
    expect(result.current.pendingNavigation).toBeNull();
    expect(window.location.pathname).toBe("/_aibox/ui/configs");
  });

  it("accepts pending dirty history navigation and guards unload", () => {
    window.history.replaceState(null, "", "/_aibox/ui/configs?current=1");
    const { result } = renderHook(() => useConsoleRouter());
    act(() => result.current.recordDirty(true));

    act(() => {
      window.history.pushState(null, "", "/_aibox/ui/sessions");
      window.dispatchEvent(new PopStateEvent("popstate"));
    });
    act(() => {
      result.current.acceptPendingNavigation();
    });
    expect(window.location.pathname).toBe("/_aibox/ui/sessions");
    expect(result.current.route.module).toBe("sessions");

    const event = new Event("beforeunload", { cancelable: true });
    act(() => {
      window.dispatchEvent(event);
    });
    expect(event.defaultPrevented).toBe(true);
  });

  it("removes history and unload listeners on unmount", () => {
    const remove = vi.spyOn(window, "removeEventListener");
    const { unmount } = renderHook(() => useConsoleRouter());
    unmount();

    expect(remove).toHaveBeenCalledWith("popstate", expect.any(Function));
    expect(remove).toHaveBeenCalledWith("beforeunload", expect.any(Function));
  });
});
