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

  it("announces busy state without changing visible text", () => {
    render(
      <RefreshButton label="Refresh Requests" busy busyLabel="Refreshing Requests" disabled>
        Refresh
      </RefreshButton>,
    );

    const button = screen.getByRole("button", { name: "Refreshing Requests" });
    expect(button).toHaveTextContent("Refresh");
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).toBeDisabled();
  });

  it("keeps icon-only refresh controls accessible without visible text", () => {
    render(<RefreshButton label="Refresh operation" iconOnly />);

    const button = screen.getByRole("button", { name: "Refresh operation" });
    expect(button).not.toHaveTextContent("Refresh");
    expect(button).not.toHaveAttribute("title");
  });
});
