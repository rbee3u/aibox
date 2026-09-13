import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RefreshButton } from "@/shared/ui/RefreshButton";

describe("RefreshButton", () => {
  afterEach(() => vi.useRealTimers());

  it("renders a contextual accessible name without a tooltip", () => {
    vi.useFakeTimers();
    render(<RefreshButton label="Refresh Tenants">Refresh</RefreshButton>);

    const button = screen.getByRole("button", { name: "Refresh Tenants" });
    expect(button).toHaveTextContent("Refresh");
    expect(button).not.toHaveAttribute("title");

    fireEvent.pointerEnter(button);
    act(() => void vi.runOnlyPendingTimers());
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  /*
   * Disabling the button while it reloads made the browser drop focus to
   * <body> on every keyboard Refresh, so the busy state is announced without
   * `disabled`: the button stays focusable and ignores a second press.
   */
  it("announces busy state without changing visible text or losing focus", () => {
    const onClick = vi.fn();
    render(
      <RefreshButton
        label="Refresh Requests"
        busy
        busyLabel="Refreshing Requests"
        disabled
        onClick={onClick}
      >
        Refresh
      </RefreshButton>,
    );

    const button = screen.getByRole("button", { name: "Refreshing Requests" });
    expect(button).toHaveTextContent("Refresh");
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button.querySelector("svg")).toHaveClass("spin");
    expect(button).toBeEnabled();
    button.focus();
    expect(button).toHaveFocus();
    fireEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("still honours the caller's disabled reasons once the reload is over", () => {
    render(
      <RefreshButton label="Refresh Requests" disabled>
        Refresh
      </RefreshButton>,
    );
    expect(screen.getByRole("button", { name: "Refresh Requests" })).toBeDisabled();
  });

  it("keeps responsive labels separate from contextual accessible names", () => {
    render(
      <RefreshButton label="Refresh operation" compactOnNarrow>
        Refresh
      </RefreshButton>,
    );

    const button = screen.getByRole("button", { name: "Refresh operation" });
    expect(button).toHaveTextContent(/^Refresh$/);
    expect(button).not.toHaveAttribute("title");
  });
});
